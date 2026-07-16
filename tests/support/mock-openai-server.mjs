import http from "node:http";

const host = process.env.CROWCLAW_TEST_HOST ?? "127.0.0.1";
const port = Number(process.env.CROWCLAW_TEST_PORT ?? "32123");
const model = "crowclaw-acceptance-model";

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
        json(response, 200, completion({
          role: "assistant",
          content: "You denied the local action. Nothing was read or run.",
        }));
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
