# Native CrowClaw Memory Foundation — Design Specification

**Status:** Proposed for Crow's review

**Date:** 2026-10-08

**Owner:** Crow

**Target:** `CrowLoki/CrowClaw-Desktop`, public non-developer edition

**Implementation branch:** `codex/native-memory-foundation`

## 1. Decision and intent

CrowClaw-Desktop will gain a native, source-aware memory foundation that can
index and retrieve its own conversations, approved actions, user notes, and
explicitly admitted file content. It will combine dependable full-text search,
the existing native CrowQuant lexical path, and an optional local semantic
embedding path.

The feature is informed by the capabilities demonstrated in Crow's separate
`crowclaw-memory`, `conversation-memory`, CrowMemory, CrowQuant, and earlier
CrowClaw projects. Those projects remain separate. Their repositories,
runtimes, databases, private data, and working directories are not runtime
dependencies and are not copied or merged into this repository.

The immediate product outcome is simple: after CrowClaw has had conversations
and completed approved work, Crow can search that history and the agent can
propose recalling relevant context without depending on another checkout or a
Python sidecar.

## 2. Non-negotiable standalone contract

The packaged CrowClaw application must satisfy all of the following:

1. All memory code, schema, migrations, search logic, and lifecycle handling
   required for the baseline feature ship inside the CrowClaw installer.
2. No operation requires a clone, path, process, service, database, or
   configuration file from another Crow repository.
3. No operation requires Python, Bun, Node.js, MCP, `sqlite-vec`, or a separate
   memory server on the installed machine.
4. No paid service, remote account, API key, or internet connection is required
   for baseline indexing and retrieval.
5. Optional semantic retrieval may use a user-configured model server on the
   local loopback interface. If that endpoint is absent, incompatible, or
   stopped, full-text and native CrowQuant lexical retrieval continue to work.
6. The app never searches Crow's machine for historical repositories, private
   corpora, identity files, or previous memory databases.
7. The public installer contains no Crow-specific corpus, credentials, account
   sessions, private research, machine paths, Orion identity material, or
   SRH-HQRE data.
8. Another project's unavailability, movement, deletion, or network outage
   cannot prevent CrowClaw from starting, opening existing conversations,
   searching baseline memory, exporting its own data, or uninstalling.
9. Existing Alpha 3 conversations, approved-action records, settings, and
   CrowQuant memories survive the upgrade.
10. A fresh-checkout packaged acceptance run must prove these guarantees before
    the feature is described as released.

The existing user-supplied local model endpoint remains CrowClaw's chat-model
boundary. That endpoint is not a dependency on another Crow project. Memory
search itself remains functional without it.

## 3. Current baseline

CrowClaw-Desktop currently provides:

- durable SQLite conversations, messages, tasks, action proposals, and action
  audit records;
- a native 256-dimensional deterministic lexical vectorizer;
- native CrowQuant-compatible 4-bit compression and compressed cosine ranking;
- manually entered CrowQuant memories;
- approval-gated `remember_memory` and `search_memory` agent tools;
- a Memory view that displays CrowQuant records and approved-action summaries;
- retention/export support that includes current CrowQuant records; and
- local OpenAI-compatible model connections.

It does not currently provide automatic indexing of conversations, FTS-backed
memory retrieval, source-aware memory provenance, semantic embeddings,
supersession/withdrawal, or rebuildable derived indexes.

## 4. Approaches considered

### A. Native in-process memory service — selected

Implement the storage, indexing, retrieval, and lifecycle behavior in Rust on
top of CrowClaw's bundled SQLite database. Reuse CrowClaw's existing approval,
task, persistence, and CrowQuant boundaries.

This is the only approach that makes core memory part of the standalone
installer while preserving offline behavior and existing Windows lifecycle
acceptance.

### B. Bundled Python memory sidecar — rejected for the core

Packaging the existing Python MCP service would initially reuse more code, but
would add Python lifecycle, `sqlite-vec` native-extension packaging, sidecar
supervision, environment repair, and a second database authority. It would
also make the core feature depend on a runtime the current product contract
explicitly avoids.

### C. External MCP-only integration — retained as a future option

An optional MCP connector may later expose an independently installed memory
system. It cannot be CrowClaw's baseline memory because the app would lose core
recall when that external service is missing.

## 5. Scope

### 5.1 Included in this foundation

- Native indexing of CrowClaw-owned conversation messages.
- Native indexing of successful approved-action summaries and bounded tool
  results that are already retained by CrowClaw.
- User-created notes entered through the Memory view or `remember_memory`.
- Explicit admission of the exact text from an already approved file read when
  the user separately chooses to remember it.
- Deterministic chunking, content hashing, incremental reindexing, and a
  rebuildable search index.
- SQLite FTS5 keyword search.
- Existing native CrowQuant lexical vectors and compressed cosine ranking.
- Optional local-loopback semantic embeddings through a bounded provider
  adapter.
- Deterministic hybrid rank fusion with source and channel explanations.
- Source type, origin reference, authorship class, lifecycle status, and model
  exposure policy for every indexed record.
- Active, superseded, and withdrawn memory states.
- Search filters, memory status, rebuild, forget, and export controls.
- Agent-tool compatibility and explicit approval before retrieved text is
  returned to the connected model.
- Crash-safe startup reconciliation and restart persistence.
- Migration and packaged acceptance from Alpha 3 data.

### 5.2 Explicitly out of scope

- Importing another project's database or scanning for it automatically.
- Watching arbitrary directories in the background.
- Indexing a selected folder merely because file-read permission was granted.
- Bundling an embedding model.
- Calling a remote embedding service in the first release.
- Automatic memory injection into every model prompt.
- Identity creation, autopoiesis, behavioral-rule generation, or source-code
  self-modification.
- CrowNest execution, SRH-HQRE operation, or Orion collaboration.
- A general MCP/plugin framework.
- Replacing CrowQuant or claiming that lexical retrieval is semantic search.
- Deleting an original conversation merely because a derived memory entry is
  withdrawn.

Those systems can build on this foundation later through explicit contracts.

## 6. Product behavior

### 6.1 First run and upgrade

For a new installation, indexing of CrowClaw-owned conversations and
user-created notes is enabled and explained during onboarding. Approved tool
content and admitted files remain opt-in.

For an existing installation, the first launch after migration presents one
plain-language choice before historical messages are indexed:

> Build CrowClaw's local memory index from conversations already stored by this
> app. This stays on this computer and can be removed or rebuilt later.

Declining leaves existing data unchanged and keeps manual CrowQuant memory
available. The choice is changeable in Settings. No other folders or databases
are offered or searched automatically.

### 6.2 Memory view

The Memory view becomes one coherent surface with:

- a search box and source filters;
- visible search mode: full text, CrowQuant lexical, semantic, or hybrid;
- result cards showing source type, date, origin label, lifecycle state, and
  why the result matched;
- a clear distinction between user statements, assistant output, approved
  tool evidence, and user-authored notes;
- semantic-service status without implying that semantic search is required;
- controls to remember a note, withdraw a memory, rebuild the index, export
  memory, and change indexing settings; and
- exact wording about whether a command removes only a derived memory entry or
  also deletes its original source through a separate existing retention flow.

### 6.3 Chat and agent behavior

The current tools remain stable at the model boundary:

- `remember_memory` stores a user-approved note in the unified memory service.
- `search_memory` searches the unified index.

The model may propose `search_memory`, but no stored text is read or returned
to the model until Crow approves the exact query, filters, and result limit.
The result includes source labels and channel scores so the model cannot
silently represent assistant-generated text as a user fact.

A user-initiated search in the Memory view does not require an agent approval;
it is a direct local read requested by the user. It does not expose result text
to the model unless Crow starts a separately approved model action.

### 6.4 Remembering approved file content

Reading a file and remembering it are separate actions. After an approved file
read, CrowClaw may offer **Remember this file content**. Accepting that action
stores the exact approved text, a content hash, the action identifier, a safe
display label, and source offsets. Declining performs no memory write.

No directory watcher or later re-read is created. If the file changes, the
stored version remains the exact admitted version until the user deliberately
admits a replacement.

## 7. Architecture

The memory system is an in-process service owned by the Rust backend:

```text
CrowClaw source transaction
  -> source admission and provenance
  -> deterministic chunking
  -> SQLite FTS index
  -> native 256-d lexical vector
  -> native CrowQuant block
  -> optional loopback semantic embedding
  -> native vector record
  -> deterministic hybrid ranking
  -> Memory UI or approval-gated agent result
```

The implementation should introduce a focused `memory` domain rather than
continuing to grow `app.rs`:

- `memory/types.rs` — source, chunk, profile, result, lifecycle, and status
  contracts;
- `memory/chunker.rs` — deterministic Unicode-safe chunking;
- `memory/embedding.rs` — bounded loopback embedding provider interface;
- `memory/indexer.rs` — admission, hashing, incremental indexing, and rebuild;
- `memory/search.rs` — FTS, lexical, optional semantic, and rank fusion;
- `memory/service.rs` — transactional orchestration and public backend API;
- `storage/memory.rs` — SQL persistence and queries; and
- existing `crowquant.rs` — retained as the native lexical vector codec and
  extended only behind tests when a later generic vector profile is admitted.

Tauri commands, agent tools, and React components consume the service through
typed contracts. They do not access SQLite directly.

The existing `crowquant_memory.rs` behavior is migrated behind the unified
service or wrapped by it. Its public behavior remains compatible during the
migration.

## 8. Storage and provenance model

Authoritative text is either an existing canonical CrowClaw row
(conversation message, approved action, or legacy CrowQuant memory) or an exact
snapshot owned by the new memory service (user note or deliberately admitted
file content). Search chunks, FTS entries, and vectors are derived and can
always be rebuilt from that authority.

### 8.1 `memory_sources`

Each admitted item records at least:

- stable source ID;
- source kind: `conversation_message`, `approved_action`, `user_note`,
  `approved_file`, or `legacy_crowquant`;
- origin identifier, such as message ID, action ID, or existing CrowQuant ID;
- storage mode: `canonical_reference` or `owned_snapshot`;
- exact admitted UTF-8 text for an owned snapshot, with canonical-reference
  sources resolving text only from the existing CrowClaw database;
- safe display title;
- authorship class: `user`, `assistant`, `tool`, or `system`;
- content SHA-256;
- created and updated timestamps;
- lifecycle state: `active`, `superseded`, or `withdrawn`;
- optional predecessor/supersession reference;
- whether the source may be returned to a model after approval; and
- source-specific metadata that contains no credential material.

Authorship class is not a truth rating. It prevents source categories from
being flattened. Assistant output remains assistant output even when retrieved
later.

### 8.2 `memory_chunks`

Each deterministic chunk records:

- stable chunk ID derived from source ID, source content hash, chunk ordinal,
  and chunker version;
- source ID and ordinal;
- exact UTF-8 text;
- start/end logical offsets;
- text SHA-256; and
- active/withdrawn state inherited from the source.

Short messages remain one chunk. Longer content is packed by paragraph into a
bounded Unicode-safe size with a bounded overlap. The exact algorithm and
version are tested so rebuilding the same source produces the same IDs.

### 8.3 `memory_vectors`

Each vector record binds:

- chunk ID;
- vector kind: `lexical` or `semantic`;
- profile ID;
- dimensions;
- codec and codec version;
- serialized vector/block bytes; and
- creation timestamp.

Lexical vectors use the current native CrowQuant block. The first semantic
implementation stores finite, L2-normalized values as little-endian `f32`
under codec ID `f32le-normalized-v1`, with 1–4096 dimensions. It does not label
those vectors as CrowQuant-compressed. The existing native CrowQuant codec is
limited to at most 256 dimensions and is not silently extended in this slice.
Generic semantic CrowQuant compression requires a later independently verified
codec, reference-profile compatibility, and golden-vector gate before
promotion.

### 8.4 `memory_embedding_profiles`

The profile records endpoint kind, model identifier, dimensions, normalization
policy, and profile digest. It never stores API keys. A profile change makes
old semantic vectors stale but does not invalidate FTS or lexical memory.

### 8.5 FTS and index state

An FTS5 table indexes active chunk text and safe titles. A small index-state
table records schema version, chunker version, current profile, last successful
source cursor, and rebuild state.

Index status is operational metadata, not memory truth. A failed index job does
not roll back the original conversation or action that was already saved.

## 9. Indexing and synchronization

### 9.1 Source capture

After CrowClaw durably commits a conversation message or successful approved
action, it schedules an in-process bounded index job. Source capture never
delays or invalidates the original transaction. A failed job remains visible
and retryable.

The job queue has one SQLite writer, bounded concurrency for local embedding
calls, cancellation, and a durable cursor. On startup, reconciliation compares
source IDs/content hashes with derived entries and resumes only missing or
stale work.

### 9.2 Incremental behavior

- Unchanged content hashes are not re-chunked or re-embedded.
- Changed admitted content creates a new source revision and supersedes the old
  active revision.
- A deleted original CrowClaw source withdraws its derived memory entries.
- A manually withdrawn memory is excluded from search immediately.
- Rebuild clears only derived FTS/vector/index state, not authoritative source
  conversations, actions, or notes.

### 9.3 Resource bounds

The implementation plan must specify and test limits for:

- maximum source bytes admitted per action;
- maximum chunk bytes and overlap;
- maximum embedding request batch and response size;
- maximum stored dimensions;
- maximum query length and result count;
- query timeout and cancellation; and
- index queue depth.

Exceeding a limit produces a visible bounded error rather than truncating
silently or consuming unbounded memory.

## 10. Retrieval and ranking

Every query can use three independent channels:

1. **FTS5** for exact words, phrases, and BM25 ranking.
2. **CrowQuant lexical** for the existing deterministic local similarity path.
3. **Semantic** when a compatible local embedding profile is healthy.

Scores from different channels are not treated as directly comparable.
CrowClaw uses deterministic reciprocal-rank fusion across available channels,
then applies source/lifecycle filters and a stable tie-break by creation time
and ID. Results expose:

- final rank;
- channels that matched;
- channel-specific rank/score where meaningful;
- source kind and authorship class;
- origin label and timestamp; and
- an exact excerpt bounded for display/model return.

If semantic search fails during a query, the query completes with FTS and
lexical results and reports semantic degradation. It does not return an empty
result while hiding the available local matches.

The initial implementation may use a bounded native scan for vectors. It must
benchmark the accepted maximum index size and cannot claim sub-second behavior
outside the measured bound. A future approximate-nearest-neighbor index is a
separate optimization, not a condition of correctness.

## 11. Embedding provider boundary

The first semantic provider adapter supports only loopback endpoints:

- OpenAI-compatible `POST /v1/embeddings`; and
- Ollama `POST /api/embed`.

Requirements:

- The endpoint host must resolve to loopback; redirects to non-loopback hosts
  are rejected.
- The user explicitly enables semantic indexing and selects the local model.
- CrowClaw displays that admitted text will be sent to that local process.
- Requests use strict timeouts, response-size limits, finite dimensions, and
  cancellation.
- Model/profile identity is recorded with each vector.
- No chat API key is silently reused for a remote embedding request.
- No automatic model download or installation occurs.
- Endpoint failure never prevents baseline memory use.

Remote semantic providers, account connectors, and paid embedding services are
outside this release.

## 12. Approval and authority boundaries

- Indexing CrowClaw-owned conversations is controlled by the memory-indexing
  setting accepted during onboarding/upgrade.
- Indexing approved-action content is separately configurable and defaults
  off.
- Remembering file contents always requires a distinct, exact admission action.
- The model cannot search or receive memory without consuming a single-use
  approval for the exact query and result bound.
- Search proposal parsing performs no memory read.
- Denial, cancellation before execution, or expired approval performs no read
  and returns no stored text.
- Result text returned to the model is also written to the existing approved
  action audit with exact source IDs.
- Memory indexing grants no permission to rerun a tool, reopen a file, contact
  a service, or act on recalled content.
- Future evolution, CrowNest, SRH-HQRE, and Orion adapters receive no authority
  merely because their source type is representable in this schema.

## 13. Migration and backward compatibility

The migration is additive and transactional:

1. Create the new memory tables and FTS structures.
2. Leave the existing `crowquant_memories` table and rows intact.
3. Create deterministic `legacy_crowquant` source/chunk references for existing
   rows without deleting or rewriting their original blocks.
4. Keep existing Tauri command payloads and agent-tool result contracts working
   while the frontend transitions to the unified representation.
5. Add new memory data to retention export/removal paths.
6. On migration failure, leave the prior schema/data usable and surface the
   exact failure; do not partially mark migration complete.

Existing CrowQuant IDs remain stable. A rebuild must not create duplicate
legacy records.

## 14. Failure and recovery behavior

- Corrupt derived vector data is quarantined from search and reported; the
  source remains available for reindexing.
- FTS corruption triggers a rebuild path, not source deletion.
- Embedding unavailability marks semantic indexing degraded and queues bounded
  retries only while enabled.
- App shutdown cancels in-flight provider calls and leaves durable source/index
  state sufficient for startup reconciliation.
- If the app stops after source admission but before derived indexing, startup
  indexes that source once.
- If it stops after vectors are committed but before job completion is marked,
  deterministic IDs make replay idempotent.
- A profile change never mixes incompatible vectors in one similarity scan.
- Export failure, reindex failure, and source deletion failure remain distinct
  user-visible outcomes.

## 15. Privacy and security

- Memory remains under CrowClaw's existing per-user application-data root.
- No telemetry or remote analytics are added.
- Credentials are never embedded in source metadata, result cards, logs, or
  exports.
- File provenance uses a safe display label, action ID, content hash, and exact
  admitted text; it does not need to expose a machine-local path to the model.
- Search excerpts are escaped and rendered as text, never executable markup.
- SQLite queries are parameterized.
- FTS query syntax is normalized or escaped rather than accepting raw SQL/FTS
  control syntax from a model.
- Embedding responses are validated for finite numeric values, exact declared
  dimensions, and bounded length.
- Memory records never become instructions merely because they were retrieved;
  the model prompt labels them as untrusted historical context with provenance.

## 16. Verification and acceptance

### 16.1 Unit tests

- deterministic Unicode-safe chunking and stable IDs;
- content hashing and unchanged-source deduplication;
- source/authorship/lifecycle validation;
- FTS query normalization;
- lexical CrowQuant round-trip and ranking;
- semantic vector validation and profile isolation;
- deterministic reciprocal-rank fusion and tie-breaking;
- supersession and withdrawal;
- limits, cancellation, and corrupt-data quarantine; and
- Alpha 3 legacy-row backfill without mutation or duplication.

### 16.2 Backend integration tests

- a committed conversation becomes searchable;
- user, assistant, tool, and note sources remain distinguishable;
- restart preserves search and resumes incomplete indexing;
- denied/cancelled agent searches do not read or expose memory;
- approved search returns exact source IDs and is audited;
- semantic endpoint outage falls back to FTS and lexical retrieval;
- profile change marks only semantic vectors stale;
- forgetting derived memory does not silently delete its conversation;
- deleting an original source withdraws derived entries;
- retention export/removal includes the new tables; and
- existing manual CrowQuant memory remains readable and searchable.

### 16.3 Frontend tests

- upgrade choice and settings behavior;
- memory search, filters, source labels, and degradation status;
- remember, withdraw, rebuild, and export controls;
- clear distinction between index-only removal and source deletion; and
- approval copy showing exact query, filters, result limit, and model exposure.

### 16.4 Packaged Windows acceptance

From a fresh checkout and clean Windows test profile:

1. Build and install CrowClaw with no sibling Crow repositories available.
2. Confirm the installed process tree contains no Python, Node.js, MCP memory
   server, or separate repository process.
3. Keep external network access disconnected, run only the deterministic
   loopback acceptance model, and leave semantic embeddings disabled.
4. Create two conversations and a user note with distinguishable subjects.
5. Verify FTS and CrowQuant lexical retrieval rank the correct records.
6. Close the app completely, reopen it, and verify the same records and source
   labels remain searchable.
7. Ask the agent to search memory, deny the proposal, and prove no stored text
   is returned to the model or audit as a successful read.
8. Approve the same search and verify exact source-bound results reach the
   model and approved-action audit.
9. Start a compatible loopback embedding endpoint, explicitly enable semantic
   indexing, rebuild, and verify a paraphrased query retrieves the intended
   source.
10. Stop the endpoint and verify baseline search continues with a visible
    degraded-semantic status.
11. Withdraw one memory and verify it disappears from search without deleting
    the original conversation.
12. Export retained data, restart, and verify index recovery.
13. Uninstall and verify application binaries are removed while user-data
    retention follows the existing uninstall choice.

No release claim is permitted until the exact packaged installer completes
this acceptance or every remaining failure is stated in release notes.

## 17. Implementation sequence

The detailed implementation plan will split work into reviewable commits, but
the intended dependency order is:

1. Storage contracts, migrations, source provenance, and legacy compatibility.
2. Deterministic chunking, FTS, lexical indexing, and rebuild/reconciliation.
3. Unified search, rank fusion, limits, and agent approval integration.
4. Memory UI, settings, lifecycle controls, export, and migration experience.
5. Optional loopback semantic adapter and profile isolation.
6. Full tests, performance bounds, packaged acceptance, and release evidence.

Each stage must leave the app buildable and preserve existing memory behavior.

## 18. Future integration boundary

This schema is deliberately capable of representing later records without
making those systems dependencies:

- governed evolution can record observations and proposed rules as distinct
  source types after its own design approval;
- CrowNest can receive explicitly selected memory packets and return attributed
  run results;
- SRH-HQRE can connect through a separately authenticated experimental adapter
  while retaining its own evidence authority; and
- Orion can remain an optional external collaborator whose contributions are
  attributed rather than bundled or impersonated.

None of those adapters is implemented by this memory slice. No future source
type may silently grant execution, publication, hardware, account, identity,
or canonical-write authority.

## 19. Reference and originality boundary

The following were inspected only to identify useful behavior and failure
modes:

- `CrowLoki/crowclaw-memory` — semantic/FTS indexing and MCP-shaped recall;
- `CrowLoki/conversation-memory` — generic conversation indexing;
- the distinct CrowMemory contracts preserved in SRH-HQRE — namespaces,
  provenance, permissions, supersession, and sealing;
- `CrowLoki/crowquant` and CrowClaw's already landed native CrowQuant
  integration — vector compression and retrieval; and
- earlier CrowClaw CLI lines — session search and autopoiesis dependencies on
  reliable memory.

No source file or Git history from those repositories is imported, merged,
cherry-picked, or repurposed. CrowClaw-Desktop receives a fresh implementation
against this product-specific specification and its own test evidence.

## 20. Review decision

Approval of this written specification authorizes creation of the detailed
implementation plan. Implementation begins only from that plan and remains on
the isolated `codex/native-memory-foundation` branch until verified and
delivered through the repository's normal review workflow.
