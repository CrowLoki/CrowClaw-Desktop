# Native memory service acceptance — 2026-10-08

This receipt covers the native memory service, its desktop command contracts,
and an actual local embedding model. It does not claim that the installed
Windows UI or installer has completed acceptance.

## Source and protocol boundary

- Branch: `codex/native-memory-foundation`.
- Previous source checkpoint: `d19a14aeb5a18188aebb1e0d53e60c1f3b0add47`.
- The semantic implementation is the next source slice on that branch.
- Database schema: 4; additive migration from Alpha 3 schema 2 preserves its
  messages, CrowQuant IDs and original compressed bytes.
- Memory remains in Rust and bundled SQLite, with no Python, Node, MCP memory
  server, sqlite-vec, donor checkout or paid route required at runtime.
- Semantic recall uses an explicitly enabled loopback profile. Offline keyword,
  exact-text and CrowQuant retrieval remain available when it is absent or
  stopped.
- Profile identity includes provider, canonical local endpoint, model identifier,
  dimensions and codec. Semantic vectors use `f32le-normalized-v1`; they are
  not mislabelled as CrowQuant-compressed.

The HTTP shapes were checked against the primary
[Ollama embed reference](https://docs.ollama.com/api/embed) and
[OpenAI embedding reference](https://developers.openai.com/api/reference/resources/embeddings/methods/create).
Only their local-compatible protocol shapes are used; the native adapter
rejects remote hosts, URL credentials, query parameters, proxy routing and
redirects. Localhost is pinned to loopback while preserving its TLS hostname.

## Fresh automated evidence

`cargo test --manifest-path src-tauri/Cargo.toml --locked` passed:

- 41 library tests;
- 23 agent/runtime harness tests;
- 17 offline memory integration tests;
- 10 semantic memory integration tests; and
- 7 storage tests.

Total: 98 passed, 0 failed. The semantic cases cover actual fixture requests,
both wire formats, reversed response indexes, profile changes, restart,
cancellation, disabling a profile while in flight, shared-service request
serialization, the full 15-second deadline, corrupt-vector quarantine/rebuild,
redirect rejection, dimension rejection, source-aware approved agent retrieval,
and retention/export. Denied proposals produce no embedding-server request.

A regression forced the final migration step to fail after schema 3 had been
created. It demonstrated that separate per-version commits left the database
at version 3 rather than its original version 2. The upgrade now has one
transaction around all intermediate steps. The regression passes: version 2,
original message text and original CrowQuant bytes remain intact, with no
partially created source tables.

Independent synthetic embedding fixtures originally produced repeatable
loopback connection deadlines when started concurrently on this Windows
runner. The same binary passed serially; the transport's underlying runtime
cause was not established. The fixtures now have exclusive ownership, matching
the app's single embedding-owner contract, while queue concurrency is tested
explicitly inside one shared service. Negative tests additionally assert that
the fixture actually received the request and that the expected protocol error
was observed, rather than accepting any unrelated transport failure.

`npm run build` passed. `npm test` passed 16/16 frontend tests. The semantic
controls verify explicit profile saving, immediate disabling during a busy
operation and preservation of an unsaved draft during status refresh. A React
event-lifetime defect discovered by those tests was corrected by capturing the
input values before state updater callbacks.

## Actual local-model canary

The installed LM Studio runtime was version `0.4.21+2`. Its inventory contained
the local GGUF Qwen3 Embedding 0.6B model; no model was downloaded. The inventory
CLI woke LM Studio, whose saved setup started its loopback API on port 1234.
CORS settings and credentials were not changed.

The model was loaded for the test under identifier
`crowclaw-memory-acceptance`, with a 4096-token context and a 120-second unload
TTL. The reported model allocation was 609.54 MiB.

The source-provided native harness was run with:

```text
cargo run --manifest-path src-tauri/Cargo.toml --locked --example semantic_acceptance -- http://127.0.0.1:1234/v1 crowclaw-memory-acceptance 1024
```

It created its own disposable database and used three synthetic notes about
vehicle servicing, vegetable irrigation and pastry ingredients. No user
profile or private corpus was read.

The native service:

1. indexed all three notes using actual 1024-dimensional model output;
2. stored three validated native vectors;
3. ranked the vehicle-servicing note first for a question phrased as an
   automobile tune-up;
4. returned a `semantic` channel with cosine score `0.7003067672026267`; and
5. reopened its database and preserved that first-ranked result.

The harness reported `accepted: true`, no warnings, and 464 ms for this small,
already-loaded-model workload. That is not a claim about cold-start latency,
large-history performance or general model quality.

The test model subsequently unloaded; the loaded-model inventory was empty.
The API was stopped and its endpoint was verified unreachable, matching the
pre-test API state. LM Studio's background GUI host remains running because
the inventory command woke it, the daemon shutdown command cannot stop a
GUI-hosted runtime, and no main window was available for a graceful close.
No forced shutdown was performed. Ollama's endpoint remained stopped.

## Remaining acceptance

- Large-history measurements at the declared scan bound.
- Actual native desktop rendering and user workflows.
- Installed upgrade/restart, export-file saving and installer/uninstaller
  acceptance with the other Crow repositories unavailable.
- Release artifact binding, review and GitHub delivery.
- The wider approved standalone roadmap, including God mode.

Source and local-model evidence alone do not satisfy these remaining gates.
