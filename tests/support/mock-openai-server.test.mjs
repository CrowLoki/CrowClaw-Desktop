import assert from "node:assert/strict";
import { test } from "node:test";
import { createModelFixture } from "./mock-openai-server.mjs";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { fileURLToPath } from "node:url";

async function fixture(t) {
  const server = createModelFixture();
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  const url = `http://127.0.0.1:${server.address().port}`;
  return {
    send: messages => fetch(`${url}/v1/chat/completions`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ model: "crowclaw-acceptance-model", messages }) }),
    evidence: async () => (await fetch(`${url}/__acceptance/memory`)).json(),
  };
}
function tool(id, result) {
  return { role: "tool", name: "search_memory", tool_call_id: id, content: JSON.stringify(result) };
}

test("denial evidence rejects retained text even if it appears outside the tool result", async t => {
  const api = await fixture(t);
  const denied = tool("call-packaged-search-deny", { state: "denied", reason: "User denied" });
  assert.equal((await api.send([denied])).status, 200);
  assert.equal((await api.send([{ role: "assistant", content: "CI native telescope cobalt record" }, denied])).status, 500);
  const evidence = await api.evidence();
  assert.equal(evidence.deniedWithoutDisclosure, 1);
  assert.equal(evidence.violations, 1);
});

test("approval evidence requires the retained note and its native provenance", async t => {
  const api = await fixture(t);
  const hit = { id: "retained-note", text: "CI native telescope cobalt record", score: 1 };
  const result = { state: "executed", output: { type: "memory_search", query: "cobalt", results: [hit] } };
  assert.equal((await api.send([tool("call-packaged-search-approve", result)])).status, 500);
  hit.provenance = { sourceId: "native-source", originId: "retained-note", sourceKind: "user_note", historicalContext: true };
  assert.equal((await api.send([tool("call-packaged-search-approve", result)])).status, 200);
  const evidence = await api.evidence();
  assert.equal(evidence.approvedSources.length, 1);
  assert.equal(evidence.approvedSources[0].sourceId, "native-source");
  assert.equal(evidence.approvedSources[0].originId, "retained-note");
  assert.equal(evidence.violations, 1);
});

test("ordinary chat remains available without enabling the packaged probes", async t => {
  const api = await fixture(t);
  const response = await api.send([{ role: "user", content: "CI baseline" }]);
  assert.equal(response.status, 200);
  assert.equal((await response.json()).choices[0].message.content, "CrowClaw acceptance response: CI baseline");
  assert.deepEqual(await api.evidence(), { deniedWithoutDisclosure: 0, approvedSources: [], violations: 0 });
});

test("the actual fixture CLI starts on its assigned loopback port", { timeout: 5000 }, async t => {
  const child = spawn(process.execPath, [fileURLToPath(new URL("./mock-openai-server.mjs", import.meta.url))], {
    env: { ...process.env, CROWCLAW_TEST_HOST: "127.0.0.1", CROWCLAW_TEST_PORT: "0" },
    windowsHide: true, stdio: ["ignore", "pipe", "pipe"],
  });
  t.after(async () => {
    if (child.exitCode === null && child.signalCode === null) {
      const closed = once(child, "close");
      child.kill("SIGTERM");
      await closed;
    }
  });
  const ready = await new Promise((resolve, reject) => {
    let output = "";
    child.once("error", reject);
    child.once("exit", code => reject(new Error(`Fixture exited before readiness: ${code}`)));
    child.stdout.on("data", chunk => {
      output += chunk;
      if (output.includes("\n")) {
        try { resolve(JSON.parse(output.split("\n")[0])); }
        catch (error) { reject(error); }
      }
    });
  });
  assert.equal(ready.ready, true);
  assert.ok(ready.port > 0);
  const response = await fetch(`http://127.0.0.1:${ready.port}/v1/models`);
  assert.equal(response.status, 200);
  assert.equal((await response.json()).data[0].id, "crowclaw-acceptance-model");
});
