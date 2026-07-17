import http from "node:http";

const host = process.env.CROWCLAW_TEST_HOST ?? "127.0.0.1";
const port = Number(process.env.CROWCLAW_TEST_PORT ?? "32123");
const model = "crowclaw-acceptance-model";

const memoryScenarios = {
  "MEMORY DENY": {
    callId: "call-memory-deny",
    name: "remember_memory",
    arguments: { text: "This denied sentinel must never be stored" },
  },
  "MEMORY QUANTUM": {
    callId: "call-memory-quantum",
    name: "remember_memory",
    arguments: { text: "Superconducting qubit calibration preserves phase coherence" },
  },
  "MEMORY GROCERY": {
    callId: "call-memory-grocery",
    name: "remember_memory",
    arguments: { text: "Grocery list with apples bread and milk" },
  },
  "SEARCH DENY": {
    callId: "call-search-deny",
    name: "search_memory",
    arguments: { query: "qubit calibration", limit: 2 },
  },
  "SEARCH QUANTUM": {
    callId: "call-search-quantum",
    name: "search_memory",
    arguments: { query: "qubit calibration", limit: 2 },
  },
};

function json(response, status, body) {
  const encoded = Buffer.from(JSON.stringify(body));
  response.writeHead(status, {
    "content-type": "application/json",
    "content-length": encoded.length,
  });
  response.end(encoded);
}

function completion(message, extra = {}) {
  return {
    id: `chatcmpl-${Date.now()}`,
    object: "chat.completion",
    created: Math.floor(Date.now() / 1000),
    model,
    choices: [{ index: 0, message, finish_reason: extra.finish_reason ?? "stop" }],
    usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
  };
}

function hasTool(request, name) {
  return Array.isArray(request.tools) && request.tools.some((tool) => tool?.function?.name === name);
}

function toolCall(callId, name, arguments_) {
  return completion({
    role: "assistant",
    content: null,
    tool_calls: [{
      id: callId,
      type: "function",
      function: { name, arguments: JSON.stringify(arguments_) },
    }],
  }, { finish_reason: "tool_calls" });
}

const server = http.createServer((request, response) => {
  if (request.method === "GET" && request.url === "/v1/models") {
    json(response, 200, {
      object: "list",
      data: [{ id: model, object: "model", created: 0, owned_by: "crowclaw-test" }],
    });
    return;
  }

  if (request.method !== "POST" || request.url !== "/v1/chat/completions") {
    json(response, 404, { error: { message: "not found", type: "not_found" } });
    return;
  }

  let raw = "";
  request.setEncoding("utf8");
  request.on("data", (chunk) => {
    raw += chunk;
    if (raw.length > 1_000_000) request.destroy();
  });
  request.on("end", () => {
    let body;
    try {
      body = JSON.parse(raw);
    } catch {
      json(response, 400, { error: { message: "invalid JSON", type: "invalid_request" } });
      return;
    }

    const messages = Array.isArray(body.messages) ? body.messages : [];
    const latest = messages.at(-1) ?? {};
    if (latest.role === "tool") {
      let result = {};
      try {
        result = JSON.parse(String(latest.content ?? "{}"));
      } catch {
        result = {};
      }
      if (result.state === "denied") {
        if (latest.name === "remember_memory") {
          json(response, 200, completion({
            role: "assistant",
            content: "You denied storing that CrowQuant memory. Nothing was stored.",
          }));
          return;
        }
        if (latest.name === "search_memory") {
          json(response, 200, completion({
            role: "assistant",
            content: "You denied searching CrowQuant memory. No stored memory was read.",
          }));
          return;
        }
        json(response, 200, completion({
          role: "assistant",
          content: "You denied the local action. Nothing was read or run.",
        }));
        return;
      }
      if (result.output?.type === "memory_remembered") {
        const memory = result.output.memory ?? {};
        json(response, 200, completion({
          role: "assistant",
          content: `CrowQuant stored memory ${memory.id}: ${JSON.stringify(memory.text)} (${memory.originalBytes} original bytes to ${memory.compressedBytes} compressed bytes using ${memory.algorithm}).`,
        }));
        return;
      }
      if (result.output?.type === "memory_search") {
        const results = Array.isArray(result.output.results) ? result.output.results : [];
        const top = results[0];
        const content = top
          ? `CrowQuant search ${JSON.stringify(result.output.query)} returned ${results.length} result(s). Top result ${top.id}: ${JSON.stringify(top.text)} with score ${top.score}.`
          : `CrowQuant search ${JSON.stringify(result.output.query)} returned 0 results.`;
        json(response, 200, completion({ role: "assistant", content }));
        return;
      }
      if (result.output?.type === "directory_listing" && hasTool(body, "read_text_file")) {
        const selected = result.output.entries?.find((entry) => /\.txt$/i.test(String(entry?.name)))?.path;
        if (selected) {
          json(response, 200, completion({
            role: "assistant",
            content: null,
            tool_calls: [{
              id: "call-read-approved-file",
              type: "function",
              function: { name: "read_text_file", arguments: JSON.stringify({ path: selected }) },
            }],
          }, { finish_reason: "tool_calls" }));
          return;
        }
      }
      if (result.output?.type === "text_file") {
        json(response, 200, completion({
          role: "assistant",
          content: `I read the approved file ${result.output.path}. Its actual contents were: ${result.output.content}`,
        }));
        return;
      }
      json(response, 200, completion({ role: "assistant", content: "The approved local action completed." }));
      return;
    }

    const content = String(latest.content ?? "");
    const memoryScenario = memoryScenarios[content.trim().toUpperCase()];
    if (memoryScenario) {
      if (!hasTool(body, memoryScenario.name)) {
        json(response, 200, completion({
          role: "assistant",
          content: `Acceptance failure: ${memoryScenario.name} was not advertised by CrowClaw.`,
        }));
        return;
      }
      json(response, 200, toolCall(
        memoryScenario.callId,
        memoryScenario.name,
        memoryScenario.arguments,
      ));
      return;
    }
    if (/which file did i approve|what was it about/i.test(content)) {
      const history = JSON.stringify(messages);
      const file = history.match(/([^"\\/]+\.txt)/i)?.[1] ?? "the approved text file";
      json(response, 200, completion({
        role: "assistant",
        content: `You approved ${file}. I retained the conversation and its approved-action result after restart.`,
      }));
      return;
    }
    const pathMatch = content.match(/\[path:(.+?)\]/i);
    const selectedPath = pathMatch?.[1]?.trim() ?? ".";

    if (/inspect|list/i.test(content) && hasTool(body, "list_directory")) {
      json(response, 200, completion({
        role: "assistant",
        content: null,
        tool_calls: [{
          id: "call-list-directory",
          type: "function",
          function: { name: "list_directory", arguments: JSON.stringify({ path: selectedPath }) },
        }],
      }, { finish_reason: "tool_calls" }));
      return;
    }

    if (/read|summari[sz]e/i.test(content) && hasTool(body, "read_text_file")) {
      json(response, 200, completion({
        role: "assistant",
        content: null,
        tool_calls: [{
          id: "call-read-text-file",
          type: "function",
          function: { name: "read_text_file", arguments: JSON.stringify({ path: selectedPath }) },
        }],
      }, { finish_reason: "tool_calls" }));
      return;
    }

    json(response, 200, completion({
      role: "assistant",
      content: `CrowClaw acceptance response: ${content || "ready"}`,
    }));
  });
});

server.listen(port, host, () => {
  process.stdout.write(JSON.stringify({ ready: true, host, port, model }) + "\n");
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.close(() => process.exit(0)));
}
