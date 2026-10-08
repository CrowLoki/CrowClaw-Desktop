import assert from "node:assert/strict";
import { test } from "node:test";
import { DatabaseSync } from "node:sqlite";
import { installedMemoryAudit } from "./installed-memory-audit.mjs";

function fixture(t) {
  const db = new DatabaseSync(":memory:");
  t.after(() => db.close());
  db.exec(`
    CREATE TABLE proposed_actions(id TEXT,tool_name TEXT,request_json TEXT,status TEXT,result_json TEXT,created_at_ms INTEGER);
    CREATE TABLE action_audit(sequence INTEGER,action_id TEXT,event_kind TEXT,detail_json TEXT);
    CREATE TABLE memory_sources(id TEXT,origin_id TEXT,source_kind TEXT,authorship TEXT);
    CREATE TABLE crowquant_memories(id TEXT,text TEXT);
  `);
  const note = "CI native telescope cobalt record";
  const provenance = { sourceId: "source", originId: "note", sourceKind: "user_note", authorship: "user", historicalContext: true };
  const hit = { id: "note", text: note, provenance };
  const result = { state: "executed", output: { type: "memory_search", query: "cobalt", results: [hit] } };
  const insert = db.prepare("INSERT INTO proposed_actions VALUES(?,?,?,?,?,?)");
  insert.run("denied-action", "search_memory", '{"type":"search_memory","query":"cobalt"}', "denied", null, 1);
  insert.run("approved-action", "search_memory", '{"type":"search_memory","query":"cobalt"}', "succeeded", JSON.stringify(result), 2);
  db.prepare("INSERT INTO action_audit VALUES(?,?,?,?)").run(1, "denied-action", "denied", '{"reason":"User denied"}');
  db.prepare("INSERT INTO action_audit VALUES(?,?,?,?)").run(2, "approved-action", "succeeded", JSON.stringify({ result }));
  db.prepare("INSERT INTO memory_sources VALUES(?,?,?,?)").run("source", "note", "user_note", "user");
  db.prepare("INSERT INTO crowquant_memories VALUES(?,?)").run("note", note);
  return { db, evidence: { violations: 0, deniedWithoutDisclosure: 1, approvedSources: [{ id: "note", ...provenance }] } };
}

test("model observation agrees with native decisions, canonical note and audit", t => {
  const { db, evidence } = fixture(t);
  assert.deepEqual(installedMemoryAudit(db, evidence, "user_note"), {
    deniedAction: "denied-action", approvedAction: "approved-action", sourceId: "source", originId: "note", sourceKind: "user_note", modelAndAuditAgree: true,
  });
});

test("denied audit disclosure cannot pass even with a clean model receipt", t => {
  const { db, evidence } = fixture(t);
  db.prepare("UPDATE action_audit SET detail_json=? WHERE action_id='denied-action'").run('{"text":"CI native telescope cobalt record"}');
  assert.throws(() => installedMemoryAudit(db, evidence, "user_note"), /Denied action\/audit contains retained text/);
});

test("source or model-observer identity mismatches cannot pass", t => {
  const { db, evidence } = fixture(t);
  evidence.approvedSources[0].originId = "another-note";
  assert.throws(() => installedMemoryAudit(db, evidence, "user_note"));
  evidence.approvedSources[0].originId = "note";
  db.exec("UPDATE memory_sources SET origin_id='another-note'");
  assert.throws(() => installedMemoryAudit(db, evidence, "user_note"));
});
