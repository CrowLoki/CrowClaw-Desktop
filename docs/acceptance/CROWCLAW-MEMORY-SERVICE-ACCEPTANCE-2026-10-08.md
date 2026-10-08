# Native memory service acceptance — 2026-10-08

This receipt covers native-service, local-model, native-UI and installed lifecycle
evidence. Earlier sections below retain the historical checkpoints; the current
result supersedes their pending basic installer/upgrade status, not their explicitly
separate release-boundary limitations.

## Current verified result

[Run 37701777105](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37701777105)
completed successfully on source `277fd3738ba489c71cf7fd9e8f515c7910bb8bb8`.
The build, fresh-install and Alpha 3 upgrade jobs each passed with zero annotations.
The logs confirm 111 Rust tests, 21 frontend tests, two verification-tooling tests,
installer-identity/policy fixtures, production builds and zero dependency advisories.

Both independent installed runners tested the same installer SHA-256
`08be67bb93cd31f2f838c43e95ec836ab6ad120b046f757871146f7251b94156` and installed
executable SHA-256 `8F9B8D6C960D3632F5D949925FB5C6D15A6D64CF904BAB67018D0384D0E814D7`.
They passed normal shortcut/onboarding, two conversations and two notes, keyword
recall with authorship labels, complete close/restart with the model fixture stopped,
native-only process ownership, and normal uninstall with database files retained
unchanged. Test-only driver policy was removed; no installation remained on either
disposable runner. Screenshots from both offline restarts were visually inspected.

The upgrade moved schema 2 to 5 with identical canonical conversation/message/note
digest `4e7178ebc5780b4e59c1e668cbeda7672e75edf4c37a23686e5fb8758df1bd4b`, preserving
two conversations, four messages and two compressed notes. Indexing consent was
explicit and persisted. Other Crow checkouts were absent from these hosted runners.
The release-publication job was correctly skipped for the manual run.

The draft-preservation executable also passed the same real native UI/Windows CMD
driver flow locally, SHA-256
`6C0EF2FB140527B331FD9CFFF5A0031AB7178DE8FF80130130E2783DE7B6947F`.
It and the model fixture are stopped. Crow's normal Alpha 3 database/WAL/SHM hashes
remain unchanged. Source delivery is tracked in [PR #3](https://github.com/CrowLoki/CrowClaw-Desktop/pull/3).
No tag, public installer release, production signing or local installed-app update
is established by this receipt.

## Source and protocol boundary at the initial checkpoint

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

## Initial source-checkpoint tests

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
- The current hosted matrix now proves the basic installed memory lifecycle,
  upgrade, native process tree and uninstall with donor checkouts absent. It does
  not prove external-network disconnection or every design section 16.4 journey.
- Complete the remaining packaged agent-approval, semantic, withdrawal and export
  journeys before claiming full release acceptance. Their service/native-executable
  evidence above is not silently promoted to the exact installed artifact.
- Public release/tag and signing/licensing decisions remain separate.
- The wider approved standalone roadmap, including God mode.

Source and local-model evidence alone do not satisfy these remaining gates.

## Later combined native candidate: evolution and personal accounts

Source `3f11ed2` adds native ChatGPT plan authentication and request transport on
top of the separately committed governed-evolution and protected account storage.
The normal full Rust test command passes, including 91 library tests and all
integration/doc-test targets. All 65 frontend tests pass across nine files.
The release-mode native build passes without creating or installing a new bundle.
Executable SHA-256:
`29F1D56768A2AD1451571D8A3F187CB3456F352045DBE65BD271C282FA8D1574`.

Verified through the actual Tauri WebView in an isolated test profile:

- Existing local-provider onboarding still connects; membership controls render
  in onboarding/settings and reject an empty registration label.
- After stopping the synthetic fixture, Connections selected the existing local
  LM Studio `antares-1b` model. Evolution recorded two real comparison responses,
  including matching requested/reported model identities.
- Draft creation, comparison and preference did not activate guidelines. The
  test comparison was rated Neither because the outputs did not follow the
  requested short format; no model improvement is claimed.
- Explicit Apply created guideline revision 1. The comparison, preference and
  active revision survived a complete application restart. Restoring revision 0
  created revision 2 without erasing prior history.
- Continue with ChatGPT reached OpenAI's CrowClaw account chooser. No account was
  selected or permission granted. Native Cancel restored the controls, stored
  no registration and closed the callback listener.

The candidate and synthetic fixture closed normally and the owned UI driver was
detached. This establishes native-executable behavior, not installation of the
newer combined candidate. Completed account authorization/token exchange, live
account catalog and inference, reconnect, quota and remote/local disconnect
journeys remain unverified. Claude integration, full packaged journeys and the
wider standalone roadmap remain outstanding. No public release is claimed.

### Reviewed session lifecycle follow-up

Independent review identified cancellation losing a rotated refresh token and
ordinary rotation cancelling healthy responses. The correction gives credential
maintenance an owned lifetime and separates durable session invalidation from
credential replacement in schema 8. Reconnect/sign-out invalidate the old session;
routine renewal does not. Session handles synchronize after another instance
reconnects. Sign-out retains cleanup ownership until the current account operation
finishes, then revokes the latest token and clears locally. Network/auth timeouts
were not increased. A real lock-contention regression failed at the earlier
30-second deadline and passes with the lock held for 31 seconds. HTTP-barrier
tests also prove renewal persistence and sign-out cleanup after caller cancellation.
The reviewer rechecked the three findings and reported no remaining defect in
that bounded cleanup review.

The updated normal Rust suite passes 172 tests: 98 library, 23 agent-runtime,
10 Evolution, 24 memory-foundation, 10 semantic-memory and 7 storage. Rust format
checking and the native release build pass; the unchanged frontend has its
65-test passing result above. Latest native executable SHA-256:
`6F5A85CBBCF313FB1564E34CAD2775014F322672B8D2595821D9DAB8EF8E9E01`.
The actual candidate upgraded the existing native test profile from schema 7 to 8.
Its full Evolution-data digest and opaque host-identity digest remained identical;
the UI retained revision 2, both model responses and the Neither preference.
Membership controls were ready, and the candidate closed normally. A separate
schema-7 regression preserves existing protected credential bytes and registration
identity through upgrade. This does not replace the live-account and packaged
acceptance gaps stated above.

### Combined installed candidate verified and source merged

[Run 37772944228](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37772944228)
tested exact source `15f03b695cb58c93db33c2883561660fbd629d04`. Windows build,
fresh installation and Alpha 3 upgrade all passed with zero annotations on each
job. Publication was skipped. Both modes tested the same installer:

- Installer SHA-256: `652f202a541d925e7475149ce29f3f086e7ad9404baa6ce8f668db7a6860d58b`.
- Unbundled executable SHA-256: `917BDB1EC0EC1EC649D2100ED72B4009EC68E4E667E9CDAB72DBBFC586DF4575`.
- Installed executable SHA-256: `36634E02E41E58F039DF8A020B6AC20C2E76CFC4BC2F82C3C970AD9D767043EE`.

The installed binary passed the exact NSIS-marker normalization check. Both
paths proved normal shortcuts, onboarding, two conversations/notes, offline
restart/recall, the native-only process tree and uninstall retaining data. Upgrade
advanced schema 2 to 8 while preserving canonical digest
`5bfb9c82baeccc9ad7dbe9fd03f9e1bfabe8a7bcbd6b6a5ad56b8adb7e7fb643`.
Both receipts confirm owned driver-policy cleanup and no remaining installation.
Evidence artifacts are `CrowClaw-native-acceptance-fresh-37772944228` and
`CrowClaw-native-acceptance-upgrade-37772944228` on that run.

[PR #4](https://github.com/CrowLoki/CrowClaw-Desktop/pull/4) was merged using Merge
at `fc2043abc37967fbd4f8bbe9674a9c086da1a6c8`; its tree matches the tested source.
This verifies the combined candidate's basic installed lifecycle, not every
packaged journey or live membership behavior. The remaining boundaries above
still apply, and no Alpha 4 tag/release was published.

### Live ChatGPT membership acceptance — 2026-10-09

Real OpenAI authorization, protected native credential storage and the selected
account's live model catalog succeeded through the actual CrowClaw executable.
The account offered both `gpt-6-luna` and `gpt-6.1-sol`; tests used Luna at low,
medium and high reasoning only. No API key, borrowed credentials, paid fallback,
alternative model or reasoning effort above high was used.

The first real requests exposed two stream compatibility defects: the successful
HTTP reply omitted Content-Type, and its final response snapshot omitted output
already emitted as completed item events. CrowClaw now tolerates the missing
advisory header while validating the bounded SSE body, and retains indexed
`response.output_item.done` events for an empty terminal output array. Valid
`response.completed` remains mandatory. Explicit non-SSE types, incomplete or
failed streams, duplicate/sparse completed items and JSON/HTML bodies do not
become successful responses. Unexpected-body diagnostics do not expose contents.

Verified on the corrected native executable:

- Luna returned the exact requested reply at low, medium and high.
- At low, Luna proposed one `search_memory` action for synthetic test-profile
  content. The native approval dialog appeared before execution. After approval,
  it received two results and reported that count correctly.
- Read-only native audit confirmed `proposed`, `approved`, `succeeded`, exactly
  one matching action, two results and a succeeded task.
- Full process restart retained the protected account, model and low effort;
  another real membership request completed without signing in again.
- All 50 membership-focused tests pass, including 10 stream/provider tests.
  The normal bundled native build and `git diff --check` pass.
- The final full Rust suite passes 175 tests (101 library, 23 agent-runtime,
  10 Evolution, 24 memory-foundation, 10 semantic-memory, 7 storage), with no
  failures. All 65 frontend tests pass; Rust format checking passes.

Executable SHA-256:
`D45DE3F3E154433DE700516F7432D58FF31BE1DFF606C426B2A2F618A73C328B`.

This supersedes the earlier chooser-only/live-inference gap, not the remaining
release gates. The exact installed artifact has not yet been tested with this
stream fix. Real token-expiry renewal, reconnect, quota exhaustion and remote
sign-out remain separate live checks; synthetic coverage is not their acceptance.
Chrome displayed `ERR_BLOCKED_BY_CLIENT` on the return page even though native
callback/token exchange succeeded; that browser rendering issue is unresolved.
Sol was available but unnecessary for these bounded tests and was not invoked.
No private account identity, credential, host identifier or machine path is
included in this receipt. No Alpha 4 release is claimed.

### Stream review and reconnect follow-up

Independent review found two compact-stream edge cases: an explicitly incomplete
item could be promoted into output, and an announced trailing item could remain
unfinished while a completed prefix was accepted. Both new regressions failed
before correction. The decoder now rejects explicit non-completed item statuses
in both item events and populated terminal output, and requires every announced
item to finish before compact reconstruction. The reviewer rechecked both fixes
and reported no remaining finding in that bounded review. All 12 response tests
pass, including the two regressions and positive announced-item completion.
The complete membership-focused suite passes 52 tests; native build, formatting
and diff checks pass.

Real saved-account reconnect succeeded without changing the registration's model
or low effort, and another request succeeded with the renewed connection. The
corrected native build then reopened that profile and passed another Luna/low
request plus another approval-gated memory search/follow-up. The native read-only
audit confirms both observed searches returned two results and each followed
proposed/approved/succeeded with a succeeded task.

Corrected executable SHA-256:
`977F0C4A37E8EDFFC4C27362E5DD81B7406C3C2EEA29103A8F21C81749FB311C`.
The previous exact-installer and remaining lifecycle/browser boundaries at that
checkpoint are superseded only by the specifically verified checks below.

### Membership delivery and native conversation composer — 2026-10-09

[PR #5](https://github.com/CrowLoki/CrowClaw-Desktop/pull/5) merged at
`5f38e245b55bb5c8705f8d99733aff2a63c3728c` after
[run 37790860065](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37790860065)
passed on exact head `c74bdf3a26b09e21a0920edaf20323c273a46da8`. Build, fresh
installation and upgrade each had zero annotations. Both installed receipts
verified artifact identity, denied search without disclosure, approved search
with matching model/native audit, uninstall retention and driver-policy cleanup.
Publication was skipped; no installation remained on the disposable runners.

- Installer SHA-256: `bc05f2385d50ea2db448e0161db50a7a30e0b532d07b7fb10820993aeab2d760`.
- Unbundled executable SHA-256: `EDF63CA173D9C6D520A3F3EC502BBBD0AA399B89E0DE46B05BA4E1F162A81326`.
- Installed executable SHA-256: `6E7F73031CF67381B42A09BF44D9ED1385C767AC66C89D154C07B8E6F8876342`.

Real native sign-out separately confirmed remote revocation, removed the protected
credential/selection and disabled sending. Reauthorizing the retained registration
restored a successful Luna/low request. During the later composer requests,
automatic credential renewal advanced the credential generation while preserving
the account session; no interactive reauthorization was required. These supersede
the earlier unverified sign-out and automatic-renewal boundaries. Quota exhaustion
was not induced; synthetic quota tests are not live exhaustion/recovery proof.

The new conversation composer passes 196 Rust tests and 106 frontend tests,
production/native build and diff checks. Independent review exposed draft-conflict,
stale sign-out and local-credential ownership defects; targeted regressions and
the integrated suite pass after correction. Each chat owns its draft and next-turn
selection; submission atomically records the message/task and clears the submitted
draft. Submitted task/reply identity does not change when later picks change.

Actual native checks established:

- Schema 8 to 9 upgrade preserved the original canonical content digest and
  existing protected account identity.
- Two chats retained independent drafts and model/effort choices through Settings,
  chat navigation and full native restart, without altering application defaults.
- Real Luna/medium and Luna/low replies used the selected chat's choice. A real
  local antares-1b response used its distinct local profile. This proves routing,
  not a model-quality improvement; the local response did not follow its format.
- Sol selection preserved drafts and prior response labels; Sol was not invoked.
  No inference test used reasoning above high or a paid API fallback.
- Final bundled executable SHA-256
  `1BA10DFC731D3912AF81D0D69AA9F7D7D09DC58A6A87A7178543E44ED5AE28F2`
  returned the exact requested Luna/low reply. Its actual bundled layout passed
  at 1180x760 and the minimum 920x640 with no horizontal overflow and Send visible.
  A further normal restart retained the reply and selection; no test draft,
  approval dialog or running task remained.

The first hosted composer run, `37812986175`, failed one frontend fixture before
installation. Its annotation showed startup awaiting a deliberately pending chat:
the fixture assumed the original chat was newest, which depended on timestamp
ties. Controlled equal/separated timestamps reproduced the latter failure. The
fixture now explicitly selects its intended initial chat; both cases exercise
the original pending-load/obsolete-failure assertions. All 107 frontend tests pass
with default CI worker settings, and the production build passes. No product code,
timeout or assertion was weakened. Exact-head hosted acceptance must rerun.

These composer results are native-executable acceptance, not yet exact-installed
artifact acceptance of this newer slice. The unresolved browser return-page
rendering, broader packaged semantic/withdrawal/export/network-isolation journeys,
Claude route and standalone roadmap remain distinct. No public release is claimed.
