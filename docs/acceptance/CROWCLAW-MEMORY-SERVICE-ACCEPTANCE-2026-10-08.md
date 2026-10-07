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

### Hosted result and reviewed source corrections

[Windows run 37646593800](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37646593800)
ran exact source `956df3f07976408ebc7c70c5441deb5d4c4de46a`. Frontend and Rust
tests, the Windows installer build and collection passed. Installation and
registration checks completed, but executable identity comparison failed before
UI acceptance. The failure annotation and step log were inspected; this run is
not a passing installed-app receipt. The retained fixture receipt binds installer
SHA-256 `b37ebc5b5c816612f74cb324afb8a4d51652b518609c0b0218316cbb88baa8cf`
and unbundled executable SHA-256
`d9be16cb81d7830f5c319d805bd3c22fd1062046c261e4bb6488403084aa9a19`.

Tauri temporarily changes an embedded bundle-type marker while packaging and
then restores the original executable. This explains why comparing the installed
NSIS payload directly with the restored build-tree executable is not the right
identity check. The corrected comparator permits only the unique NSIS marker
change, in memory; any other byte difference still fails. It does not patch files
or disable bundler behavior. See [Tauri's bundler implementation](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs).
Fixture tests reject unrelated mutations, missing/wrong/ambiguous markers and
input mutation. Installed execution of that corrected gate remains to be run.

Further local transport comparison reproduced 32/32 failures on ports 9517–9548
with each of HTTP, Tokio TCP and ordinary blocking TCP. HTTP and blocking TCP
controls on ports 19894–19925 passed 32/32. The controls include an overlapping
run. No app database or memory logic participates in this probe. Local policy
observations were read-only; no firewall/sandbox settings, retries, fixed test
ports or serial-only gate were introduced as a product fix.

Independent production review resulted in these source corrections:

- Native note identity and canonical CrowQuant insertion share one transaction;
  forced identity-write failure rolls back the canonical insert.
- Stale legacy candidates cannot install a second active note identity. Either
  identity withdraws the origin. Schema 5 repairs existing one-sided exclusions,
  removes their chunks/vectors, retires duplicate active aliases, and enforces
  alias withdrawal at the database boundary without deleting original notes.
- Reset/rebuild queues admitted-file jobs transactionally. Reopen can resume
  those jobs. Recovery keeps the admitted source identity and recorded hash;
  changed or missing snapshots become visible bounded job failures, not new
  revisions falsely attributed to the earlier approval.
- Search fetches only metadata for its bounded participating source IDs, never
  all historical file snapshot bodies. A corrupt unrelated historical snapshot
  no longer prevents otherwise valid search.
- The native export projection now includes canonical retained notes,
  conversations and action provenance plus memory settings. A withdrawn note
  and an unindexed conversation remain in export; unrelated settings stay out.

Each reported defect was reproduced by a failing regression before its fix.
The latest normal full run passes 111 Rust tests: 47 library, 23 agent, 24 memory,
10 semantic and 7 storage. Frontend 18/18, production frontend build,
release-script checks, installer identity fixtures and the local refusal guard
pass. The earlier native export receipt proves saving/parsing only; it does not
retroactively prove this expanded export payload in the installed app.

### Still requiring product acceptance

### Hosted launch follow-up and upgrade verification

[Run 37651817821](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37651817821)
tested `c24ae4c4b790513e8dc23b6f71243aa40fa8defb`. Its normal Rust gate passed
111 tests. Build, installation, registry identity and normalized executable
identity passed. The native UI driver then timed out waiting for CDP on 9227;
no UI or uninstall pass is claimed. The job log and failure annotation were read.
The fixture receipt binds these SHA-256 values:

- installer: `bea4314e727cde804419e35f4bb56992af61d5a7683780d0c2791da8bfd957ed`;
- unbundled executable: `1d19d48561ba3b23903bdb32b00857bbc6bb4630e3f5b1c90ef0375e4a6ab6ca`;
- installed executable: `2db98b4017a222b64319ccfc392c3eebc4a5dac26bde6f057fa23604f2f18b7b`.

A local two-mode probe launched the previously verified diagnostic executable
through a private shortcut and directly, each with a new synthetic profile.
Both showed the WebView debugging flag and the onboarding accessibility element;
both were closed. No installer registration was changed, and all three normal
database hashes still matched. This disproves a universal shortcut-inheritance
failure; the hosted cause remains unconfirmed.

The driver now keeps normal shortcut/UI proof separate from explicit child-process
CDP setup. Only the automation browser cache is isolated; SQLite uses the normal
installed-app profile. This follows the supported [WebView2 automation boundary](https://playwright.dev/docs/webview2)
and explicit [.NET child-environment handling](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.processstartinfo.environment).
Failure receipts record app exit/window state, owned process names and whether
the WebView debug flag was present, without dumping command lines or credentials.

Fresh installation and upgrade are distinct matrix entries on separate clean
hosted runners, using one exact built artifact. The upgrade baseline is published
Alpha 3 source `7874429d0316667584589e40213e91cc2e210f8d`, installer SHA-256
`c46b2e646410bbb620a20fb94ae743d1538d9093aaf6b043b6bab58b578feb74`, verified against
its published manifest and downloaded bytes. The baseline download token is not
present in the application-launch step. A tag-only publication job depends on
both acceptance modes; manual runs cannot publish releases.

The upgrade receipt hashes all canonical conversation/message/note fields, with
integer-to-text conversion before JavaScript to preserve 64-bit values. Tests
detect changed ordering, metadata, IDs, compressed blocks and seeds, while allowing
the derived schema version to advance. UI checks positively inspect both threads,
both notes and the original user-message search result. Only the upgrade-consent
phase may click the indexing choice; a repeated prompt on restart fails.

Uninstall uses normal NSIS self-removal and waits for both executables, registration
and shortcuts to disappear before comparing retained database hashes. It does not
manually delete leftovers to manufacture a pass. See the [NSIS command-line contract](https://nsis.sourceforge.io/Docs/Chapter3.html).

Local verification: 18 frontend tests, 2 Node verification-tooling tests, frontend
build, installer identity fixtures, embedded-JavaScript/PowerShell syntax, local
execution refusal, and actionlint 1.7.12 all pass. The new hosted launch and upgrade
paths still require execution; these preparation results are not installed-app
acceptance. The independent harness review was integrated and closed.

### Outstanding acceptance

### Reviewed schema-5 native follow-up

The current release executable was rebuilt from the product source shared by
`8466fac` and `2ce34a1` (their difference is test policy and checkpoint text),
SHA-256 `1CE3CA6CB2BFE8EF179D6040A446554E324DD520866D4D4937D2A16A0033BD93`.
It opened only the existing synthetic Alpha 4 profile and migrated that profile
to schema 5. The actual WebView/native Save dialog exported 106,493 bytes with
SHA-256 `9C45D26CA6C92E5E1707E1D3ECBD3F0306AED6E88B51A7D2CE39F86CC3EC1032`.
The parsed export contains two canonical notes, one retained conversation with
its messages, seven actions, ten tasks and nineteen audit entries, alongside
memory settings and derived source data. The withdrawn quantum note remains
present as a canonical original; unrelated settings are absent. The app showed
the native export-success notice.

With the model fixture stopped, the real UI rebuilt the index, returned no
`user_note` result for the withdrawn coherence note, and returned the original
approved basil file snapshot through keyword search. The screenshot was visually
inspected. The app then closed normally. All three normal Alpha 3 SQLite/WAL/SHM
hashes still matched the pre-test baseline. This closes the expanded native-export
gap above; it is not a new installed-artifact or real-model acceptance claim.

### Hosted elevated-launch repair

Run [37660605623](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37660605623)
passed the build, but both fresh/upgrade receipts showed a normal native window
and no requested WebView debugging flag. The same failure affected the unchanged
published Alpha 3 baseline. [GitHub's Windows runner privileges](https://docs.github.com/en/actions/reference/runners/github-hosted-runners#administrative-privileges)
and [Microsoft's elevated WebView host rules](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security#for-an-elevated-host-app-use-appropriate-override-flags)
explain why explicit child environment flags alone are insufficient. The harness
uses only app-specific HKLM test policy on elevated disposable runners, verifies
an owned loopback listener, and removes its own values after each launch/failure.
Non-elevated hosts retain the child-environment route. No product flags, local
registry changes, runtime downgrade or security-control changes were introduced.

Independent review caught shared-key recreation by `New-Item -Force`; the corrected
in-memory registry test reproduced the loss before the fix. Policy creation now
preserves existing keys/other applications. The reviewer rechecked the remedy
and closed with no remaining findings. Run `37698159044` was cancelled during
build before installed acceptance. Replacement run
[37698471213](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37698471213)
tested `2ce34a1`. Its build and source tests passed with no build annotations.
Both installed runners confirmed elevation, WebView `153.0.4234.48`, the requested
debug flag, owned loopback CDP, normal onboarding, and removal of the test policy.
The upgrade runner also created two Alpha 3 conversations, four messages and two
notes, then launched the upgraded Alpha 4 UI. This proves the original CDP failure
is repaired. Both jobs subsequently failed on the same multiline UI assertion
program with `SyntaxError: Unexpected token ')'`; their failure annotations and
logs were inspected. Neither job passed the complete installed lifecycle.

The latter error was reproduced through the actual Windows `npx.cmd` wrapper:
an inline multiline program is truncated at the shell boundary even though the
original text passes JavaScript syntax checks. The driver now uses Playwright
CLI's supported `run-code --filename` input. The exact formerly failing program,
with an early read-only sentinel, parsed/executed through `npx.cmd` using a script
path containing spaces. No CLI/runtime downgrade or changed app assertion was
used. The same onboarding, two-conversation/two-note seeding, source/provenance
assertions and offline restart passed through the actual CMD wrapper in a new
isolated native profile. Both the app and fixture server were stopped afterward.
This does not exercise local installer registration: the hosted full-lifecycle
rerun remains required.

### Remaining acceptance boundaries

Run [37699937792](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37699937792)
on `badac23` passed build and upgrade acceptance. Both held the candidate installer
SHA-256 `955f7a83ab8394e5da7fc1b3d41cac97c395e21174de445591d9e82a7890e598`;
installed executable SHA-256 was
`13DE510391F1E4AFCE1BEBDF332D4EC951695FA65DA598135993FD35F30B5AB5`.
The upgrade receipt preserves the exact same canonical digest across schema 2 to 5:
`cf9864698e2db40481d5892878a7dea7b346eb4b8ce82615ee162b12af1ec247`.
It passed normal shortcuts for both versions, indexing consent, two conversations,
two notes, provenance, offline restart, native-only process tree, normal uninstall
with retained data, and test-policy cleanup. Build and upgrade annotations were empty.

Fresh acceptance failed while entering the next note: the submit button stayed
disabled after an earlier save cleared the newer draft. The log/annotation were
inspected, and a deterministic deferred-save regression reproduced the lost input.
The app now clears only the draft belonging to the completed save. Newer edits and
failed-save text are retained; 21 frontend tests and the production build pass.
The harness also waits for a saved note card, not editable textarea text, as proof
of storage. This is an app correction plus a stronger test assertion, not a delay
or retry masking the race. The corrected packaged candidate still needs both modes.

- The native debug-build benchmark at the 10,000-chunk bound completed on
  2026-10-08: 26,971 ms indexing, 614 ms keyword search, 843 ms lexical search,
  and 740 ms combined search. Each query ranked the known synthetic source
  first. The database file was 13,565,952 bytes. These measurements are for
  this synthetic native-service workload, not UI latency or a general SLA.
- Reconcile remaining native model-connectivity evidence with the isolated
  host-port behavior; preserve explicit degraded/offline behavior and never
  weaken host security controls as a product workaround.
- Complete the clean installed-profile matrix (including separate conversations
  and full runtime-dependency isolation). This host was not disconnected from
  external networking, and other checkouts were not hidden/moved.
- Installed upgrade and installer/uninstaller acceptance with the other Crow
  repositories unavailable. The NSIS candidate was built but not installed;
  existing Alpha 3 registration and shortcuts were preserved.
- Release artifact binding, review and GitHub delivery.
- The wider approved standalone roadmap, including God mode.

Source and local-model evidence alone do not satisfy these remaining gates.
