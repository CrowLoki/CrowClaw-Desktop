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

### Native candidate checkpoint (2026-10-08)

The first Alpha 4 candidate was built from `1e504f0`. Its NSIS SHA-256 was
`b76f34bd6bf0395f047f802550a738577d2523c69dea6434c362c026151984b5`;
the release executable SHA-256 was
`a3b31d2918f0d619a4cedd8acfbE68cb29661b8bb222ffd529de82d2f14cf667`.
The executable, not the installer, was launched with a separate SQLite/WebView
profile. Alpha 3 registration, shortcuts and the normal profile were not used.

Playwright attached to the real Tauri WebView2, not a development adapter. A
synthetic loopback test model supplied deterministic tool calls and embeddings;
that model is fixture evidence, not a model-quality claim. Windows UI Automation
operated only the candidate's native folder dialog.

Verified in that native window:

- Remember denied: no denied-sentinel CrowQuant row; two approved notes stored.
- Memory view: filtered user-note recall, authorship and semantic match labels.
- Semantic settings: explicit fixture profile, three dimensions, stored vectors.
- Agent recall: denied search returned no memory result; approved search returned
  the quantum note as the top source-bound result.
- User-selected fixture folder: separate directory-list and exact-file approval;
  the response contained the actual basil-before-sunrise fixture text.
- Tasks: cancel during the delayed provider response reached `Cancelled` with
  zero active tasks. Two earlier delayed runs completed before automation could
  click Cancel; those are not counted as cancellation acceptance.

Native export **failed**: WebView2's Downloads Hub displayed `Couldn't download`.
The source fix uses a native Save dialog and a staged/synced file followed by
rename; cancellation writes nothing, and the UI reports success only after save.
The source test checks replacement and preservation on a failed destination.
Rebuilt native export acceptance is still pending at this checkpoint.

The frontend suite now passes 18 tests, including export cancellation/success and
background-count refresh/unmount cleanup. The production frontend builds.
Vitest is updated to 5.0.3 and vulnerable build/test transitive packages are
patched; `npm audit --audit-level=low` reports zero advisories.

The default Rust run passed library (46), agent (23), and offline memory (18)
targets, but failed these three semantic tests:

- `cancellation_and_disable_discard_inflight_embeddings` (fixture-request wait);
- `semantic_paraphrase_retrieval_persists_and_offline_search_survives_outage`
  (embedding connection deadline);
- `shared_service_serializes_requests_and_cancellation_releases_queue`
  (fixture-request wait).

The unchanged test binary passed semantic 10/10 with `--test-threads=1`, and the
whole suite then passed 104/104 with that setting, including storage 7/7. This
does not resolve the default gate. The native app also intermittently timed out
against its still-running fixture; later requests succeeded without a restart.
Three direct Node HTTP probes to the same listener returned 200 in 16/3/2 ms.
These observations do not establish the Windows transport root cause. The
release wrapper retains its normal test command; no failure is hidden by changing
it to a serial run.

### Rebuilt native candidate results

`npm run tauri build` succeeded from clean commit
`08d4a04a9b6f16242abf91dc89290f5320b6e46f`. This was a diagnostic candidate build,
not a passing release-wrapper run (the default-test failure above remains).

- Executable SHA-256:
  `bcea4cd1b9a18c8cff95b57e764e2b4578dc7e76ec433582b47dbd892818ec68`.
- NSIS SHA-256:
  `d58ce798044310d5234377a631fb0f598ef551880403f946e6631cb2e8cddd23`.
- Native Save dialog wrote a 75,178-byte JSON file: schema 4, 21 sources,
  21 chunks, 21 vectors. The UI showed success after the file existed and parsed.
- A second native dialog was cancelled; the UI reported cancellation, with one
  export file still present. No browser download was used.
- After a complete application close/reopen, existing conversations, approved
  notes, profile settings and semantic results remained available.
- Stopping the owned synthetic model produced a visible connection-refused
  warning and a keyword/CrowQuant fallback result for the quantum note.
- Withdrawing that note removed it from keyword search after rebuild and another
  restart. Read-only inspection confirmed the original CrowQuant row/text still
  existed. The denied sentinel still had zero stored rows.
- The synthetic file was changed after its approved read. Clicking `Remember
  this approved file content` stored the earlier approved basil text, not the
  replacement. A source-filtered keyword query found that snapshot before and
  after restart; the unapproved replacement had zero indexed chunks.
- Normal Alpha 3 SQLite, WAL and SHM hashes all matched their captured baseline.
- The candidate was closed gracefully and the exact owned synthetic server was
  stopped. No test app/server was left running. The local isolated profile,
  synthetic export and screenshots were retained for reproducibility.

Local screenshots under ignored `output/playwright/` were visually inspected:
`native-before-rebuild.png`, `native-offline-fallback.png`, and
`native-file-memory-after-restart.png`. They are test-profile evidence, not
public release assets. The earlier ignored `release/` directory still contains
the first candidate's manifest; the hashes above refer to the rebuilt files in
`src-tauri/target/release/` and its `bundle/nsis/` subdirectory.

### Still outstanding

### Follow-up transport investigation and hosted acceptance preparation

The semantic fixture now records accepted connections, received-byte counts,
complete-request counts and elapsed time on failure. With the old global fixture
lock retained, one normal semantic run and 12 bounded nine-test repetitions passed.
The normal full 104-test run also passed while frontend tests/build ran alongside
it. The lock was then removed: it had not resolved the earlier failures and was
unnecessary coupling between otherwise independent fixtures.

Without that lock, a bounded repetition reproduced eight failing semantic tests.
Five fixtures had zero accepted connections, zero request bytes and zero complete
requests at failure; three received only the first request. This narrows those
failures to connection establishment/acceptance, not vector decoding or ranking.
It does not identify an OS, library or security-product cause. A later normal
full run passed 104/104, still without serial flags or a fixture lock.

The standalone `loopback_probe` example compares 32 requests each using reqwest
HTTP, Tokio TCP and standard blocking TCP, with an independent bounded standard
TCP server and no app database. An initial probe-server flaw (accepted Windows
sockets inherited nonblocking mode) was corrected before interpreting results.
The corrected comparison passed 32/32 in all three modes. Earlier probe failures
from that fixture flaw are not evidence of the application defect.

The existing Windows workflow now includes `Test-InstalledCrowClaw.ps1`, guarded
to fresh GitHub-hosted Windows only. It uses real installed UI and verifies both
conversations and notes across restart, then uninstalls and compares retained
database hashes. Local parse and refusal checks passed. The harness was reviewed
independently read-only; all three material findings were verified and addressed.
Hosted execution remains pending. No local installation/account state changed.

### Remaining product gates

- The native debug-build benchmark at the 10,000-chunk bound completed on
  2026-10-08: 26,971 ms indexing, 614 ms keyword search, 843 ms lexical search,
  and 740 ms combined search. Each query ranked the known synthetic source
  first. The database file was 13,565,952 bytes. These measurements are for
  this synthetic native-service workload, not UI latency or a general SLA.
- Resolve the default parallel semantic test failures and intermittent app
  loopback timeouts without replacing the test gate with a serial retry.
- Complete the clean installed-profile matrix (including separate conversations
  and full runtime-dependency isolation). This host was not disconnected from
  external networking, and other checkouts were not hidden/moved.
- Installed upgrade and installer/uninstaller acceptance with the other Crow
  repositories unavailable. The NSIS candidate was built but not installed;
  existing Alpha 3 registration and shortcuts were preserved.
- Release artifact binding, review and GitHub delivery.
- The wider approved standalone roadmap, including God mode.

Source and local-model evidence alone do not satisfy these remaining gates.
