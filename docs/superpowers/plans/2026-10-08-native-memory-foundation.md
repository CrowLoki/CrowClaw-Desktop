# Native CrowClaw Memory Foundation Implementation Plan

> **For agentic workers:** Execute task by task with the code-work skill. Crow authorized implementation in this chat; the primary agent owns integration and verification.

**Goal:** Deliver the memory foundation described in the approved design as part of the standalone Windows application.

**Architecture:** Rust owns one SQLite database, source provenance, chunks, FTS and native vectors. Derived indexes are rebuildable; canonical messages and existing CrowQuant rows are preserved. Optional loopback embeddings enhance recall while offline FTS and lexical retrieval remain available.

**Tech Stack:** Existing Tauri 2, Rust, bundled SQLite/rusqlite, React/TypeScript; SHA-256 for stable content identity.

**Spec:** `docs/superpowers/specs/2026-10-08-native-crowclaw-memory-foundation-design.md`

## Global constraints

- No other Crow repository, Python, Node, Bun, MCP memory server or sqlite-vec is required by the installed memory feature.
- Preserve Alpha 3 IDs, blocks, messages, approvals and settings.
- No private corpus, external database discovery or automatic file watching.
- Search proposal parsing and denied/cancelled execution perform no memory read.
- Only explicitly enabled local loopback embeddings; no automatic downloads.
- Source records and retrieved text are historical data, never instructions.
- Keep source and packaged acceptance separate; record remaining gaps honestly.

## Review focus

- Unicode and symbol-only messages: preserve original text and offsets even when lexical vectorization is unavailable.
- Withdrawal and deletion: rebuild and restart must not resurrect forgotten entries or retain searchable deleted conversation text.
- Disabled indexing: upgrade choice must prevent indexing past messages until accepted.
- Large histories: bound each index batch and expose pending work rather than silently truncating it.
- Embedding failure/profile change: preserve offline search and reject mixed, corrupt or incompatible vectors.

## Task 1: Native storage and offline retrieval

Files: `storage/migrations.rs`, `storage/memory.rs`, `storage/models.rs`, `storage/retention.rs`; new `memory/{mod,types,chunker,service,search}.rs`; `Cargo.toml`, `lib.rs`.

Interfaces: `MemoryService::new(Arc<Storage>)`; `settings()`, `configure(MemorySettings)`, `sync(limit, cancellation)`, `search(MemoryQuery, cancellation)`, `withdraw(source_id)`, `rebuild(cancellation)`, `status()` and `export()`.

Limits: source 1 MiB; chunk 2048 UTF-8 bytes, overlap 256 bytes; index batch 64 sources; search query 4096 bytes, limit 1–20; vector scan 10000 chunks with explicit limit error; one SQLite writer; reciprocal rank fusion constant 60. Semantic vectors 1–4096 dimensions, normalized f32 little endian.

- [x] Add integration tests for conversation/note recall, role provenance, legacy compatibility, upgrade opt-in, restart, supersession, withdrawal across rebuild, original-source deletion and export/removal.
- [x] Demonstrate missing behavior with the focused test target before implementing.
- [x] Add transactional schema 3 with source/chunk/job/exclusion tables and external-content FTS triggers. Retain original schema 2 CrowQuant rows unchanged.
- [x] Implement deterministic Unicode-safe chunks, canonical-source admission, stable hashes and durable incremental jobs.
- [x] Implement parameterized FTS and CrowQuant rankings, fusion, filters, corrupt-vector diagnostics and cancellation.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --locked --test memory_foundation` and storage regression tests; format/check the diff.

## Task 2: Desktop and agent integration

Files: `app.rs`, `lib.rs`, `tools/types.rs`, `tools/definitions.rs`, `crowquant_memory.rs`; gateway contracts/implementations, `MemoryView.tsx`, `App.tsx` and frontend tests.

- [x] Expose typed memory status/settings/search/withdraw/rebuild/export/admit-file commands.
- [x] Schedule bounded index work after canonical writes and reconcile pending work on startup.
- [x] Route approved agent searches through the unified service while preserving remember-action recovery and exact result/source attribution.
- [x] Add the upgrade choice and local memory controls; preserve the existing CrowQuant interface and its tests.
- [x] Admit only the retained result of a successful approved file read when the user separately chooses to remember it.
- [x] Verify denied search, cancellation, source labels, model exposure, migration choice and rebuild through backend/frontend public seams.

## Task 3: Optional local semantic retrieval

Files: new `memory/embedding.rs`, vector persistence and profile contracts; Memory view configuration/status.

- [x] Implement OpenAI embeddings and Ollama embed with loopback-only URL admission, no proxies, redirects disabled, 15-second timeout and 4 MiB response bound.
- [x] Validate response model/profile, finite values, dimensions, normalization and byte length; batch at most 8 chunks per request.
- [x] Store profile-bound native f32 vectors; ignore stale profiles and expose degraded status on failures.
- [x] Verify paraphrase retrieval with a deterministic local fixture, failure fallback, cancellation, remote URL denial, redirect denial and profile change.

## Task 4: Integrated delivery and acceptance

- [x] Run focused tests, all affected Rust/frontend regressions, production build, release-script checks and `git diff --check`.
- [x] Benchmark the chosen scan bound without claiming results outside the measured workload.
- [ ] Build the installer and exercise design section 16.4 against a clean profile with donor checkouts unavailable; preserve installed user data.
- [x] Record exact passing/failing source, packaged, restart, migration and semantic gates in this checkpoint.
- [x] Review the branch and deliver the verified source/receipts through PR #3 using Crow's saved settings; its linked GitHub record owns integration status.

## Current checkpoint

2026-10-08: Crow confirmed the goal is to finish CrowClaw-Desktop and requested God mode integration too. Tasks 1–3 are implemented on `codex/native-memory-foundation`: native source/index storage and controls, approved agent recall, plus optional local embeddings with schema 5, profile isolation, offline fallback and cancellation. A failed-upgrade regression proves that all intermediate schema changes roll back together while original messages/CrowQuant bytes survive. The actual installed Qwen3 Embedding 0.6B model passed native paraphrase/restart acceptance using synthetic notes and a disposable database; no download or private corpus was used. Exact boundaries and superseded test checkpoints are retained in [the service acceptance receipt](../../acceptance/CROWCLAW-MEMORY-SERVICE-ACCEPTANCE-2026-10-08.md).

Current evidence includes passing source/local-model, native-executable and basic installed-memory lifecycle checks. It is not full release acceptance. Native UI acceptance exercised the actual Alpha 4 executable in its separate profile: note approval/denial, memory search approval/denial, source-filtered semantic recall, native folder selection, separate directory/file approvals, actual fixture-file content, and cancellation from Tasks. Browser-blob export failed in WebView2; it was replaced with a native Save dialog and atomic file publication. Status refreshes while background indexing runs without loading every stored chunk just to count it.

Current source checks: 21 frontend tests, production frontend build and dependency audit (zero advisories) pass. The latest normal Rust run passes 111 tests, without serial flags or global fixture serialization. Local transport diagnostics reproduced failures outside the app on one port range with HTTP, async TCP and ordinary blocking TCP; a control range passed. Host networking/policy settings were inspected read-only and left unchanged. The fresh hosted Windows run also passed the normal suite. Do not change security controls or force test ports/serial retries to conceal the local behavior.

The rebuilt executable and NSIS candidate at `08d4a04` built successfully. Native Save produced a parsed schema-4 JSON file; cancel wrote no additional export. The same isolated profile passed semantic recall after restart, visible offline fallback after the fixture server stopped, withdrawal across rebuild/restart without deleting the original note, and remembering the retained approved file contents after the actual file had changed. That remembered snapshot remained searchable after a second restart. All three normal Alpha 3 database/WAL/SHM hashes remained unchanged. The candidate and synthetic server are now stopped; the test profile, export and local screenshots remain available. Exact hashes and evidence are in the existing acceptance receipt.

Hosted run [37646593800](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37646593800) on `956df3f` passed source tests/build and installed the exact NSIS candidate. It stopped before UI acceptance because the installed executable differed from the unbundled build-tree hash. Logs and the failure annotation were inspected. Tauri's bundler changes one bundle-type marker and restores the original build-tree file afterward; the identity check now normalizes only that unique NSIS marker in memory and rejects every other byte difference. Positive/negative identity fixtures, PowerShell parsing, embedded JavaScript parsing and the local refusal guard pass.

Independent production review found four memory gaps and two recovery edge cases. They now have failing-before/passing-after regressions: canonical note identity commits with the note; aliases share withdrawal; schema 5 repairs old one-sided exclusions/chunks/vectors transactionally; admitted-file rebuild jobs survive interruption while preserving source ID/hash and rejecting missing/changed snapshots; search fetches bounded metadata without historical snapshot bodies; export includes retained canonical originals and memory settings, excluding unrelated settings. A reviewer suggestion to suppress the chat-model denial response was rejected against spec section 12: denial forbids the denied data access, not reporting its denial to the already-connected model. Both review assignments are closed.

Hosted run [37651817821](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37651817821) on `c24ae4c` passed all 111 Rust tests, build, install, registration and the corrected binary-identity check. It failed waiting for WebView CDP on port 9227. The failure log, annotation and receipt were inspected. A bounded local probe using only separate synthetic profiles showed both shortcut and direct launches carrying the debug flag and exposing onboarding; that does not establish the hosted failure's cause. The updated harness proves the normal shortcut/default-profile UI through process-scoped UI Automation, then starts the same installed binary with an explicit child-only CDP environment and separate browser cache. The real default SQLite profile remains shared across app restarts. Failure receipts now record sanitized process/launch diagnostics.

Hosted run [37660605623](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37660605623) on `c7c26ce` passed the build job, but both fresh/upgrade jobs failed waiting for CDP. Both receipts prove that the normal shortcut rendered the native UI and that the subsequent WebView process lacked the requested debugging flag. This also affected the unmodified published Alpha 3 baseline. GitHub documents that hosted Windows runners run as administrators; Microsoft documents that elevated WebView2 hosts ignore environment/HKCU overrides but honor app-specific HKLM policy. The test harness now uses that documented mechanism only on elevated disposable runners, rejects existing policy, verifies a process-owned loopback listener, and removes its own values after every launch/failure. It records actual elevation/runtime versions. No product code, local registry, runtime version or security control was changed for this repair. Local policy safety fixtures, identity/syntax/refusal checks, verification-tooling tests and actionlint pass; the hosted fix is not yet verified.

Independent review of the elevated-driver repair found that registry `New-Item -Force` can erase unrelated values in a shared policy key. The in-memory double was corrected to model the documented behavior and reproduced the loss. The harness now creates only missing ancestors/keys without `-Force`; existing policy is preserved and the regression passes. The reviewer rechecked the fix and closed with no remaining findings. Preparation run `37698159044` is confirmed cancelled before installed acceptance. Replacement run `37698471213` tested `2ce34a1`: source tests/build passed; both installed runners proved the original WebView/CDP failure repaired, including app-specific policy cleanup. Both then failed on a multiline UI script truncated by Windows `npx.cmd`. That exact syntax error was reproduced locally; the driver now uses supported `run-code --filename` transport. The unchanged conversation/note/provenance assertions and offline restart subsequently passed through the actual CMD wrapper in a fresh isolated native profile. Hosted full-lifecycle rerun remains the next gate.

The latest local release executable was rebuilt with schema 5 and passed native Save/export of retained canonical notes, conversations, actions, tasks and audit records. A withdrawn note remains in export but not search after rebuild; the approved basil-file snapshot remains searchable with the model fixture stopped. The test app closed normally and normal Alpha 3 SQLite/WAL/SHM hashes remain unchanged. Exact artifact hashes and limitations are in the existing service acceptance receipt. This is current native-executable evidence, not a substitute for the pending installer lifecycle run.

Run [37699937792](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37699937792) on `badac23` passed the build and the complete Alpha 3 upgrade lifecycle: schema 2 to 5 with identical canonical data, explicit consent, two conversations/notes, source labels, offline restart, native-only process tree, uninstall retaining data and policy cleanup. Build/upgrade annotations were empty. Fresh acceptance found a separate note-entry race: a completed save could clear the next draft entered while it was pending. A deterministic deferred-save frontend regression failed before the fix. MemoryView now clears only the submitted draft, preserving newer text; all 21 frontend tests and the production build pass. The test's note checkpoint now requires a saved card rather than matching editable textarea content. Fresh installed acceptance of this repair remains pending.

Verified stopping point: [run 37701777105](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37701777105) passed on `277fd37`. Build, fresh-install and Alpha 3 upgrade all passed with zero annotations. Both modes proved two conversations/notes, source-labelled keyword recall, offline restart, the native-only process tree and normal uninstall with retained data. The upgrade preserved the exact canonical digest across schema 2 to 5. Both used the same installer; exact hashes and screenshots are bound in the existing acceptance receipt. A local native fresh-profile/restart run also passed the corrected note UI, and all normal Alpha 3 database hashes remain unchanged. No local installer ran.

Source integration is tracked in [PR #3](https://github.com/CrowLoki/CrowClaw-Desktop/pull/3); use its merged source before the next capability slice and preserve historical branches/uncommitted work. Next unfinished product work is the approved standalone roadmap below, plus the remaining full packaged journeys in design section 16.4 before any release claim. Basic installed lifecycle success is not substituted for packaged agent-approval, semantic, withdrawal/export or external-network-disconnection evidence. Alpha 4 release notes still block publication. The old dependency-security worktree and Crow's installed Alpha 3 remain untouched. God mode remains outstanding in the full app goal.

Verification boundary: the candidate is version `0.1.0-alpha.4` and supports
explicit `--profile-dir` isolation for SQLite and WebView data. Startup rejection
and the 10,000-record native benchmark passed; exact evidence is in the receipt.
Alpha 3 remains installed on Crow's host. Do not replace its registration or open
its normal database merely to repeat candidate verification; use an isolated
profile and the hosted installer lifecycle as already established.

## Next standalone capability slices

### Current governed-evolution continuation — 2026-10-08

Memory/CI delivery is merged in PR #3 at `b713bf1`. Evolution source is preserved
at local commit `290e129` on `codex/governed-evolution`; the active stacked branch
is now `codex/membership-providers` in the same existing attached checkout. Native schema 6,
terminal-task feedback, manual/model proposals, two-response comparisons, explicit
Apply/Reject, immutable revisions/restore, new-task guideline injection and
revision attribution, global TaskCenter cancellation, and export/removal are
implemented in the current source. They use no donor runtime or account state.
The frontend passes 40 tests and its production build. Ten focused native evolution
tests pass, including original reflection inputs after later feedback edits,
reported-model identity, concurrent Apply arbitration, full removal and pre-request
serialized-context bounds. Independent review findings are resolved. Both helpers
are closed. Native UI/provider and packaged acceptance are still unfinished.

The normal Cargo suite exposed a test-executable missing Common Controls v6
manifest (`STATUS_ENTRYPOINT_NOT_FOUND`). `build.rs` now embeds the dependency in
non-binary linked targets while preserving Tauri's existing binary manifest. The
Windows manifest extractor confirms the unit executable's v6 dependency, and the
original test command now starts/runs its tests. Production/native manifest and
installed acceptance must still be exercised for this change.

The subsequent IPv4 loopback transport failure remains open. It reproduces in
ordinary blocking TCP and an independent .NET probe on the same port, before any
CrowClaw HTTP/model code. IPv6 works on that same port. A scoped Packet Monitor
trace reports discarded synthetic IPv4 handshake packets; firewall rule removal,
forced test ports and a protocol switch have not been used as a fix. A temporary
worker-mode experiment did not fix it and restored the original active inline
mode. Owned capture/filter cleanup is verified; all diagnostic helpers are stopped.
No persistent network/security setting was changed.

WFP inspection identifies the installed Npcap 0.9982 packet-capture driver from
2019 as an active loopback component. Its involvement is not yet proven causal.
No accessible Packet.dll/wpcap.dll capture client was detected; this is not proof
that no raw driver handles exist. The official Npcap 1.89 installer is downloaded
privately and its Nmap Software LLC Authenticode signature verifies; it has not
been executed. Next repair decision is a supported driver update/isolation with
its licence and effects on existing capture tools understood, then the original
IPv4 probe and normal Cargo suite must pass before dependent acceptance resumes.
Private packet/WFP artifacts remain ignored in `.test-runtime`; do not publish
machine/account data or turn this hypothesis into a verified root-cause claim.

The coordinated membership addition is now part of this full goal: each person's
own ChatGPT/Claude connection, independent install host ID, separated protected
registration/account credentials, account-bound model/effort choices, and actual
desktop request/reconnect/disconnect/quota acceptance. Preserve all existing
providers. Do not transfer Embodiment credentials, import conversations/memory,
share Crow's membership or use paid API fallback. Official direct protocol sources
are [SIWC overview](https://developers.openai.com/siwc/token-sharing-open-source),
[registration](https://developers.openai.com/siwc/token-sharing-open-source/sign-in),
and [models/inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).
The other project's released harness evidence is provenance, not CrowClaw's
implementation or live acceptance. CrowClaw remains the desktop exception; phone
and hardware work stays with those independent projects.

### Current account-storage continuation — 2026-10-08

The original linked failure in run `37660605623`, fresh job `112931795647`, was
rechecked through GitHub's job log: WebView's acceptance endpoint on port 9227
never became ready. Later run `37701777105` passes build, fresh install and Alpha 3
upgrade, with zero annotations on all three jobs. PR #3 is confirmed merged at
`b713bf181a769a921493298fa66ca25433566152`. The old failed run remains historical
evidence; it is not a current failing product gate.

Native-only membership groundwork is implemented locally on
`codex/membership-providers`, stacked on the preserved Evolution commit. Schema 7
adds one profile-owned UUIDv4 host identity and separately labelled account/client
registrations. Windows current-user DPAPI protects token records, with immutable
provider/issuer/subject/client/host binding; email is display metadata, not a key.
No raw-token IPC, browser token storage, source credential import, real sign-in or
provider request was added. Claude token import is explicitly refused; its own
supported runtime/profile still needs implementation.

SQLite immediate transactions arbitrate host creation and credential generations
across processes. Compare-and-swap rejects stale replacement/catalog completion,
including late results after local sign-out. Sign-out retains the registration
and host mapping while clearing protected tokens and active choices. Catalogs and
model/effort selections are account-bound. Ordinary token replacement preserves
that registration's choices; a fresh catalog clears choices no longer offered,
rather than transferring them to another account. This is a validated native
storage contract, not a claim that provider catalogs have been fetched live.
Memory/content exports exclude the membership vault and registrations; explicit
full data removal clears them. Existing settings/providers remain unchanged.

Fresh verification: `cargo test --locked --lib membership` passes 10 Windows tests,
including real DPAPI round-trip/tamper/binding, same-email separation, restart,
cross-install refusal, concurrent replacements, late-result rejection, selection,
export/privacy, retention and schema-6 upgrade. The affected integration command
passes 24 memory-foundation, 10 Evolution and 7 storage tests. Frontend tests pass
40/40 and the production frontend build passes. `git diff --check` passes.
The native provider functions are deliberately not wired yet and produce dead-code
warnings in non-test builds; do not suppress these or call membership connected.

A pre-accept review reproduced token replacement unnecessarily clearing an
account's chosen model/effort. A new regression failed before the change; the
replacement operation now preserves same-registration choices, while sign-out
and unavailable-model catalog refresh still clear them. All ten membership tests
pass after the change, including the formerly failing regression. The post-fix
integration targets were rerun without the membership name filter: 24 memory,
10 Evolution and 7 storage tests pass with zero tests filtered out. These are
focused source checks; they do not replace the still-open full Cargo/network gate.

The native release build completed. An initial manifest check incorrectly assumed
Tauri had declared DPI/OS compatibility. Direct inspection of the installed
`tauri-build` 2.6.3 source proves its default manifest contains only Common Controls
v6, matching the extracted executable manifest. No DPI/compatibility declarations
were lost, and no new platform declaration was added. The inaccurate build-script
comment was corrected; verification must compare to the actual Tauri default,
not an invented requirement. A structure/attribute comparison passes for every
manifest declaration; the initial literal XML comparison differed only in
whitespace. Native version resources still report CrowClaw `0.1.0-alpha.4`.
This is manifest/build evidence, not installed UI acceptance of the current
Evolution/account candidate. The latest-source native release rebuild passes;
its extracted manifest again matches the complete current Tauri default. Executable
SHA-256: `E96F907E09A4D49992955ADB80845C53F8F93663E9F43D05E01BABB6021F62FD`.
The build reports 17 dead-code warnings from the not-yet-wired native provider
seams. No installer, account connection, public release or installed-app update
was performed for this groundwork. It remains on the local stacked source branch;
the canonical main checkout and its three user-owned dirty paths are unchanged.

Next required provider work: signed OIDC validation and native PKCE/callback flow,
serialized rotating-token refresh (storage CAS alone does not serialize remote
refreshes), account-specific catalog/effort parsing, Responses streaming through
its terminal event, cancellation/quota/revocation handling, the supported Claude
route, settings UI and real intended-request/restart/disconnect acceptance. The
official [account/session contract](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)
and [model/inference contract](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
govern those boundaries. No API-key or paid-credit fallback is permitted.

The independent IPv4 transport fault above is still unresolved; normal full Cargo
and provider/native acceptance are not claimed passing. Crow's specifically
reserved Npcap update decision has not been answered; no driver update, firewall
change or test-port substitution was performed. The completed offline groundwork
does not bypass that repair requirement. The full app goal remains active.

Carry Crow's approved roadmap forward after memory: governed recursive evolution; skills and capability support; CrowNest council/laboratory capabilities; evidence and experiment handling informed by SRH-HQRE; advanced CrowQuant; optional attributed collaborator support. Core installed behavior must operate without donor projects or private state.

Crow additionally requested CrowClaw God mode on 2026-10-08. The matching candidate inspected read-only is `Crow-GodMod3`, whose current `CONTINUATION.md` records model pools, separate runtime profiles, diagnostics and text/image/audio modes. Its source includes AGPL-3.0 material and corresponding-source requirements, and its previous integration design was an optional plugin. Select a concrete native/offline module design before integrating it; do not make CrowClaw launch its website, require its checkout/gateway, or silently choose a different CrowClaw software licence. The God mode request remains outstanding in the full app goal.
