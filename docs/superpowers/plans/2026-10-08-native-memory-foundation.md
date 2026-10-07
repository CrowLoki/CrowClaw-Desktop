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

2026-10-08: Crow confirmed the goal is to finish CrowClaw-Desktop and requested God mode integration too. Tasks 1–3 are now implemented on `codex/native-memory-foundation`, extending source checkpoint `d19a14a`: native source/index storage and controls, approved agent recall, plus optional local embeddings with schema 4, profile isolation, offline fallback and cancellation. The full Rust suite passes 98 tests and the frontend passes 16 tests plus production build. A failed-upgrade regression also proves that all intermediate schema changes roll back together while original messages/CrowQuant bytes survive. The actual installed Qwen3 Embedding 0.6B model passed native paraphrase/restart acceptance using synthetic notes and a disposable database; no download or private corpus was used. Exact boundaries, fixture-runtime behavior and temporary-runtime cleanup are recorded in [the service acceptance receipt](../../acceptance/CROWCLAW-MEMORY-SERVICE-ACCEPTANCE-2026-10-08.md).

This is a source/local-model checkpoint, not installed-app or release acceptance. Native UI acceptance has now exercised the actual Alpha 4 release executable in its separate profile: note approval/denial, memory search approval/denial, source-filtered semantic recall, native folder selection, separate directory/file approvals, actual fixture-file content, and cancellation from Tasks. Browser-blob export failed in WebView2; it has been replaced in source with a native Save dialog and atomic file publication. Status now refreshes while background indexing runs without loading every stored chunk just to count it.

Current source checks: 18 frontend tests, production frontend build and dependency audit (zero advisories) pass. All 104 Rust tests pass with `--test-threads=1`, but the default run failed three semantic tests on loopback deadlines despite fixture serialization. The app also had intermittent model-connection failures. Their root cause remains unverified; do not replace the normal release gate with a serial run or call this fixed. Next: rebuild the candidate and verify native export/save/cancel, restart, withdrawal/rebuild and visible offline fallback, then investigate the transport gate and finish installer-lifecycle acceptance. Main and the separate dependency-security branch remain untouched. Continue the wider approved standalone roadmap after memory delivery.

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
