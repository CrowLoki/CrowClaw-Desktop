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

- [ ] Run focused tests, all affected Rust/frontend regressions, production build, release-script checks and `git diff --check`.
- [x] Benchmark the chosen scan bound without claiming results outside the measured workload.
- [ ] Build the installer and exercise design section 16.4 against a clean profile with donor checkouts unavailable; preserve installed user data.
- [ ] Record exact passing/failing source, packaged, restart, migration and semantic gates in this checkpoint.
- [ ] Review the branch, deliver through the existing GitHub workflow using Crow's saved settings, and continue the agreed standalone roadmap from merged source.

## Current checkpoint

2026-10-08: Crow confirmed the goal is to finish CrowClaw-Desktop and requested God mode integration too. Tasks 1–3 are implemented on `codex/native-memory-foundation`: native source/index storage and controls, approved agent recall, plus optional local embeddings with schema 4, profile isolation, offline fallback and cancellation. A failed-upgrade regression proves that all intermediate schema changes roll back together while original messages/CrowQuant bytes survive. The actual installed Qwen3 Embedding 0.6B model passed native paraphrase/restart acceptance using synthetic notes and a disposable database; no download or private corpus was used. Exact boundaries and superseded test checkpoints are retained in [the service acceptance receipt](../../acceptance/CROWCLAW-MEMORY-SERVICE-ACCEPTANCE-2026-10-08.md).

This is a source/local-model checkpoint, not installed-app or release acceptance. Native UI acceptance has now exercised the actual Alpha 4 release executable in its separate profile: note approval/denial, memory search approval/denial, source-filtered semantic recall, native folder selection, separate directory/file approvals, actual fixture-file content, and cancellation from Tasks. Browser-blob export failed in WebView2; it has been replaced in source with a native Save dialog and atomic file publication. Status now refreshes while background indexing runs without loading every stored chunk just to count it.

Current source checks: 18 frontend tests, production frontend build and dependency audit (zero advisories) pass. The latest normal Rust run passes 111 tests, without serial flags or global fixture serialization. Local transport diagnostics reproduced failures outside the app on one port range with HTTP, async TCP and ordinary blocking TCP; a control range passed. Host networking/policy settings were inspected read-only and left unchanged. The fresh hosted Windows run also passed the normal suite. Do not change security controls or force test ports/serial retries to conceal the local behavior.

The rebuilt executable and NSIS candidate at `08d4a04` built successfully. Native Save produced a parsed schema-4 JSON file; cancel wrote no additional export. The same isolated profile passed semantic recall after restart, visible offline fallback after the fixture server stopped, withdrawal across rebuild/restart without deleting the original note, and remembering the retained approved file contents after the actual file had changed. That remembered snapshot remained searchable after a second restart. All three normal Alpha 3 database/WAL/SHM hashes remained unchanged. The candidate and synthetic server are now stopped; the test profile, export and local screenshots remain available. Exact hashes and evidence are in the existing acceptance receipt.

Hosted run [37646593800](https://github.com/CrowLoki/CrowClaw-Desktop/actions/runs/37646593800) on `956df3f` passed source tests/build and installed the exact NSIS candidate. It stopped before UI acceptance because the installed executable differed from the unbundled build-tree hash. Logs and the failure annotation were inspected. Tauri's bundler changes one bundle-type marker and restores the original build-tree file afterward; the identity check now normalizes only that unique NSIS marker in memory and rejects every other byte difference. Positive/negative identity fixtures, PowerShell parsing, embedded JavaScript parsing and the local refusal guard pass.

Independent production review found four memory gaps and two recovery edge cases. They now have failing-before/passing-after regressions: canonical note identity commits with the note; aliases share withdrawal; schema 5 repairs old one-sided exclusions/chunks/vectors transactionally; admitted-file rebuild jobs survive interruption while preserving source ID/hash and rejecting missing/changed snapshots; search fetches bounded metadata without historical snapshot bodies; export includes retained canonical originals and memory settings, excluding unrelated settings. A reviewer suggestion to suppress the chat-model denial response was rejected against spec section 12: denial forbids the denied data access, not reporting its denial to the already-connected model. Both review assignments are closed.

Next unfinished action: push the corrected source and rerun the guarded fresh-Windows installed acceptance, then inspect all results and annotations before branch delivery. Local `.test-runtime/hosted-acceptance-state.json` records the exact active run when dispatched; verify it against GitHub, not just that local receipt. The harness checks registration, Start menu launch, two distinct conversations, both notes, conversation-source recall, restart/offline recall and uninstall retention. It refuses non-hosted runners and prior CrowClaw NSIS/MSI state. Installed upgrade and the full packaged memory matrix remain open. Alpha 4 notes retain publication-blocking placeholders. No local installer has been run or Alpha 3 registration replaced. Main and the separate dependency-security branch remain untouched. Continue the wider approved standalone roadmap after memory delivery; God mode remains included, not yet integrated.

Native acceptance preparation: the candidate is version `0.1.0-alpha.4` and
supports explicit `--profile-dir` isolation for both SQLite and WebView data.
Four startup tests pass, including rejection of invalid paths without falling
back to the normal profile. The 10,000-record native benchmark passed; measured
times are in the acceptance receipt. Alpha 3 is installed on this host, so do
not replace its installer registration or open its normal database during the
candidate test. Build the candidate, exercise its actual WebView against a
separate profile, then determine the remaining installer-lifecycle gate from
the available isolated environment. Normal database hashes were captured
locally before testing.

## Next standalone capability slices

Carry Crow's approved roadmap forward after memory: governed recursive evolution; skills and capability support; CrowNest council/laboratory capabilities; evidence and experiment handling informed by SRH-HQRE; advanced CrowQuant; optional attributed collaborator support. Core installed behavior must operate without donor projects or private state.

Crow additionally requested CrowClaw God mode on 2026-10-08. The matching candidate inspected read-only is `Crow-GodMod3`, whose current `CONTINUATION.md` records model pools, separate runtime profiles, diagnostics and text/image/audio modes. Its source includes AGPL-3.0 material and corresponding-source requirements, and its previous integration design was an optional plugin. Select a concrete native/offline module design before integrating it; do not make CrowClaw launch its website, require its checkout/gateway, or silently choose a different CrowClaw software licence. The God mode request remains outstanding in the full app goal.
