# CrowClaw product contract

## Concrete product

CrowClaw is Crow's original creation. Within this repository, CrowClaw-Desktop
is the canonical public, non-developer edition: a Windows desktop AI-agent
application with its own product identity, direction, and release history.

A person installs it, launches it normally, connects a local model, chats,
authorises tool actions, gives it tasks, closes it, reopens it without losing
the conversation, and can update, repair, or uninstall it.

It is not a website, research explainer, command-line-only program, read-only dashboard, renamed SRH-HQRE interface, or public doorway to Orion or Crow's accounts.

## Alpha 1 required experience

1. Install CrowClaw on Windows without Python, Rust, Node.js, or source code being present on the target machine.
2. Launch CrowClaw from the Start menu.
3. Complete onboarding by selecting a detected or manually entered local OpenAI-compatible endpoint:
   - LM Studio;
   - Ollama;
   - llama.cpp server;
   - another user-supplied compatible endpoint.
4. Start a conversation and receive a real model response.
5. Ask CrowClaw to inspect a user-selected folder and summarise a text file.
6. See the proposed file/tool action before it runs and explicitly approve or deny it.
7. Close CrowClaw, reopen it, and continue the same conversation with its prior messages intact.
8. View and cancel an in-progress task.
9. Change model connection and permission settings.
10. Uninstall CrowClaw without deleting user data unless the user explicitly chooses removal.

## End-to-end acceptance test

From a fresh Windows installation:

1. Install and launch CrowClaw.
2. Connect it to a local test model through an OpenAI-compatible endpoint.
3. Create a conversation and send: `Inspect the folder I select, list its text files, and summarise the file I approve.`
4. Select a fixture folder containing two text files.
5. Verify CrowClaw presents the exact proposed read action and does not read before approval.
6. Approve one file read and verify the response uses its actual contents.
7. Close the application completely and relaunch it.
8. Ask: `Which file did I approve and what was it about?`
9. Verify the correct conversation, approved action, filename, and answer persist.
10. Uninstall and verify application binaries are removed while retained user data follows the uninstall choice.

No release may claim this test passes unless it has been executed against the packaged installer.

## Explicitly separate work

SRH-HQRE research, Orion identity and continuity, CrowMemory, CrowNest, CrowQuant, NotebookLM, YouTube, TikTok, GitHub account control, public research sites, sensors, quantum experiments, and hardware interfaces are not silently included in Alpha 1. Future integrations require their own contracts, scoped credentials, and Crow's explicit direction.

## Open decisions

These are not guessed by the implementation:

- final software licence;
- final signing identity and production certificate;
- whether a future release embeds a model;
- which additional accounts or systems Crow chooses to connect;
- any public Orion experience;
- any scientific claim or research-publication decision.

## Native governed evolution

Crow authorized a native learning loop as part of the standalone roadmap. Its
owner is CrowClaw's own SQLite database and Rust agent runtime.

1. Review actual completed, failed and cancelled agent tasks and record explicit
   quality feedback. Execution success alone is not a quality score.
2. Write a proposal locally or explicitly ask the connected model to reflect on
   one selected task. Preserve the submitted goal, original feedback/evidence,
   requested model selector and reported model identity.
3. Compare a typed evaluation prompt against the proposal's baseline and exact
   candidate guidelines, using one frozen connection without tool execution.
   Record both responses and reported identities; distinguish a model mismatch
   or unavailable identity. Only the user assigns a response preference.
4. Inspect/edit and explicitly apply or reject the proposal. Applying creates a
   new immutable guideline revision. A stale base cannot overwrite newer work.
5. New agent tasks include the adopted guidelines and record their revision;
   existing task sessions retain their original revision. Guidelines do not
   change native tool permissions or issue approval tokens.
6. Restore earlier guidelines by creating another revision, preserving history.
7. Keep reflection/comparison work visible and cancellable through Tasks even
   after leaving Evolution. Cancellation wins before result publication; durable
   result and task completion commit together.
8. Preserve the data across restart, include it in native retained-data export,
   and remove it when the user explicitly chooses full app-data removal.

No source-code/weight self-modification, donor checkout, memory sidecar or paid
service is introduced by this learning loop. A saved proposal or test comparison
is not an automatic claim of improved intelligence.

## Personal membership providers

Crow additionally authorized each independent desktop install to connect its
user's own eligible ChatGPT or Claude membership, while retaining existing model
providers and local operation. Every install owns a stable opaque host identifier;
account/workspace/client registrations and protected credentials remain separate.
Model and reasoning-effort choices must come from and remain bound to the selected
account. Reconnect/disconnect, quota/error handling and an actual intended request
are acceptance requirements. No other project's sign-in, token, chat or memory is
imported, and no API-key/paid-credit fallback is authorized for membership access.

ChatGPT uses the released public Sign in with ChatGPT/Responses protocol. Claude
membership requires a supported official runtime/route for this desktop client;
Console/API billing is not a Pro replacement. Personal local-client eligibility
does not establish an owner-subscription service for remote public customers.
These are provider additions to implement and verify, not a current connected
provider claim.

## CrowClaw desktop workspace expansion — authorized 2026-10-09

Crow wants the breadth of the Windows ChatGPT/Codex working experience expressed
as CrowClaw's own standalone product: both the visible controls and the runtime
capabilities behind them. This is a behavioral target, not permission to copy
another application's implementation, identity, private services or user state.
It adds to the approved memory, Evolution, CrowNest and God mode roadmap.

### Chat and model control

- Put account/provider, model and reasoning choices at the conversation composer.
  Permit a model change during an existing conversation without discarding its
  history, draft or attachments. Changes apply to the next submitted turn, never
  retroactively to a running or approval-paused task.
- Each conversation owns its next-turn choice; each submitted task retains an
  immutable account/provider/model/effort snapshot. Display requested/reported
  model identity where available. Switching chats must not silently transfer a
  different account or model choice into another chat.
- Show models and effort levels actually offered by the selected account/provider.
  Support the range through ultra when offered, not a universal hard-coded list.
  Do not map unsupported settings to a different effort silently. Missing metadata
  means unknown/provider default, not invented capability. A stale catalog or
  disconnected account must produce a recoverable error, not another billing route.
- Crow's current agent-operated membership tests remain limited to Luna first,
  Sol 6.1 if needed, low/medium/high only. The product's selectable capabilities
  are not restricted by this temporary testing policy.
- Add attachments with explicit selection, removal, preview, type/size limits and
  provenance. Support modalities only through a verified provider/tool capability;
  attaching a file must not silently upload an entire directory or unrelated data.
- Preserve accessible keyboard interaction, IME-safe Enter, multiline composition,
  draft recovery, send/stop/cancel states and action-specific approvals.

### OpenRouter — added to the standalone goal 2026-10-09

- Provide a distinct native OpenRouter connection with live model-catalog retrieval
  and model selection in the existing conversation composer. Do not substitute a
  hard-coded catalog or treat an OpenRouter key as ChatGPT membership credentials.
- Refresh and validate the selected model against current provider metadata; expose
  available modalities and reasoning settings only when supported by that metadata
  and verified by the adapter. Handle unavailable/changed models and catalog errors
  explicitly, preserving drafts and attachments without silent model substitution.
- Reuse conversation-owned next-turn selection and immutable submitted-turn identity.
  Protect the user's own connection credentials; never copy another application's
  credentials into CrowClaw or expose keys in messages, exports or logs.
- Preserve existing free-only/no-paid-fallback constraints. Adding OpenRouter is not
  authorization to purchase credits, use paid inference, or route a failed request
  through a chargeable model. Its availability must not become a standalone-core
  prerequisite.
- Free classification applies to the complete requested operation, including
  output generation, per-request, image, audio, video, processing/plugin and any
  other applicable charges. Zero prompt/completion rates alone do not establish
  free media output. Unknown or incomplete pricing must not receive a free badge
  or silently become an allowed free-only request. Verify modality-specific
  endpoint prices and supported capabilities separately.
- Crow additionally requires optional paid-model compatibility, without using his
  key or credits for paid development/testing. Keep his saved connection free-only.
  Paid operation is a separately enabled user choice with that user's own profile,
  credentials and explicit spending authorization; never an automatic fallback.
  Use public metadata and synthetic fixtures to develop paid paths without making
  paid inference requests. This requirement is not a claim of implemented paid mode.
- Acceptance includes live catalog retrieval, selection/refresh/error recovery,
  account/model isolation, and an actual allowed model request in the native app.

### OpenAI plan route and optional API services — clarified 2026-10-09

- Scope the preview limitations below to the specific Responses route. The
  separate image-generation transport requires its own user-authorized grant;
  a SIWC access token for api.openai.com/v1 must not be sent to the Codex image
  endpoint. Its image
  adapter requires a distinct Codex authorization explicitly linked to the selected
  registration by its signed verified email. Each grant retains its own validated
  issuer, client, subject and account-routing identity inside DPAPI, with immutable
  linkage to the primary registration. Subjects from different OAuth clients are
  not required to match; explicit conflicting account IDs are still rejected.
  Every user authorizes their own grant. Do not copy another product's credentials
  or silently fall back to a paid API key.
  Native CrowClaw generation, approval audit, atomic PNG/message storage, preview
  and restart were accepted locally on 9 October. The requested low/1024x1024 test
  returned a 1254x1254 PNG; this does not prove every size/quality control is honored.
  Image editing and a public release remain unaccepted.
- Sign in with ChatGPT plan usage supports eligible Responses requests with
  namespace-grouped function/custom tools or additional_tools input items, and web
  search when permitted by the selected model and account/workspace policy.
- Text, image and file inputs are supported only where the selected model accepts
  them. Audio/video input, Files API upload, transcription and hosted image
  generation are outside this preview route. Hosted file search, Code Interpreter,
  native computer use, hosted MCP/connectors and Responses tool_search are also
  unsupported; do not equate those hosted restrictions with a ban on local tools.
- CrowClaw's existing native agent runtime remains the single execution owner.
  Local file/command/application tools, MCP clients and agent coordination can use
  supported function/custom calls with the existing approvals and lifecycle.
  Provider adapters must not introduce a second body/agent engine, identity,
  memory store or duplicate execution loop merely to support another modality.
- GPT-Live, API transcription/STT and TTS are separately billed optional adapters,
  not benefits silently granted by membership login or requirements for the core.
  Their paid development/testing must not use Crow's keys or credits without new
  explicit spending authorization. Local or independently verified no-cost routes
  remain possible within the same runtime.
- Follow current preview request requirements: store:false, stream:true, explicit
  input history, supported tool shapes and omission of unsupported request fields.
  Documented support does not establish that CrowClaw has implemented/tested it.
- Preserve dated, model-specific empirical results when live behavior differs from
  preview documentation. On 2026-10-09, Crow-authorized GPT-6 Luna/low tests through
  CrowClaw's own membership service accepted and enforced max_output_tokens at 16
  and 32 (minimum16), accepted flat and namespaced function calls, completed a
  namespaced local-tool result round-trip, accepted a namespaced custom call, and
  executed web search. Explicit temperature/top_p and the other tested unsupported
  fields were rejected. Do not remove demonstrated capabilities solely from a
  blanket documentation assumption, or generalize one account/model result to all
  models. Custom/hosted output handling still needs native adapter/UI integration.

### Workspace and navigation

- Left navigation: projects/workspaces, grouped chats, spaces/collections,
  schedules, skills/plugins and connected capabilities. Register existing folders
  by reference; never move, merge or duplicate the user's project files to create
  a project entry. Keep project/account ownership and histories distinct.
- Right work area: independently selectable, resizable and closable panels for
  files/artifacts and previews, browser, terminal, source/diffs/review, task/tool
  activity and related chats. Panels preserve useful state across navigation;
  closing a panel is not implicit cancellation or deletion of the underlying work.
- Top navigation: native File/Edit/View/Help actions, back/forward, sidebar/panel
  toggles and keyboard shortcuts, backed by real commands and navigation state.
- Account area: sign-in/out, selected account, provider-authoritative usage and
  reset information where exposed, settings, help and optional CrowClaw pet
  controls. Unknown usage is shown as unavailable; never invent a remaining quota.

### Extensible capabilities and execution

- Native CrowClaw skill, plugin, hook and MCP management: discover/configure,
  enable/disable, inspect capabilities/permissions, invoke, cancel, diagnose and
  remove through supported lifecycle operations. Do not presume another product's
  plugin package, marketplace or subscription-only tool works unchanged here.
- One native capability registry drives both tool availability and relevant UI
  entries. Distinguish not installed, disabled, disconnected, unsupported, ready,
  busy and failed. A new connection adds only the surfaces it actually supplies.
- Local application/file/command control, browser work, remote/SSH connections,
  scheduled/background work and richer artifacts require their own verified
  adapters, scoped credentials, visible activity and recovery. A membership login
  authorizes eligible model inference; it does not itself supply these runtimes.
- Persist projects, conversations, drafts, turn choices, schedules, capability
  configuration and panel layout under CrowClaw's native ownership. Keep protected
  credentials separate from content exports. Extensions/hooks cannot silently
  change native approval boundaries or inherit other applications' credentials.
- Local operation and existing model providers remain available. No paid service,
  donor checkout, Codex installation or private account is a prerequisite for the
  standalone core. External optional integrations declare their own requirements.

### Delivery and acceptance

Deliver in coherent vertical slices: composer/turn choice; attachment lifecycle;
workspace/navigation/panels; extension registry and skill/MCP/hook execution;
scheduling/remote/application adapters; account/usage/help/pet controls. Reuse the
same native owners rather than introducing separate databases for each surface.
The existing checkpoint owns detailed order and active work; this is not a claim
that these capabilities are already implemented.

Each delivered control must exercise its real backing operation, preserve scope
and state through restart, and expose actionable failure/recovery. Verify keyboard
and narrow-window behavior, running-task/model-change isolation, cross-account
and cross-project separation, cancellation and disable/revoke transitions. Missing
vendor functionality or account eligibility is a stated boundary, not a hidden
fallback or a decorative enabled control. Public release truth remains governed
by the installer acceptance rules above.

## Alpha 2 scoped CrowQuant integration (historical acceptance)

The Alpha 1 boundary above remains part of the release history. Alpha 2 adds one bounded integration to CrowClaw itself: a local CrowQuant-compatible memory path. It does not import, rewrite, or modify the separate CrowQuant repository.

The Alpha 2 experience must:

1. Let the user enter text to remember from CrowClaw's Memory surface.
2. Convert that text to a deterministic local vector and store it as a compressed CrowQuant-compatible block alongside the source text and required metadata.
3. Let the user enter a query and show ranked, relevant stored records from the local database.
4. Show enough user-visible detail to establish that the memory was stored and retrieved, including compression information.
5. Preserve those records and their retrieval after CrowClaw is closed and reopened.
6. Operate in the installed application without Python, a paid service, a remote account, a source checkout, or the separate CrowQuant repository.

### Alpha 2 packaged acceptance test

From a fresh Windows installation:

1. Install and launch the Alpha 2 CrowClaw installer.
2. Open Memory and store at least two meaningfully different text records.
3. Verify CrowClaw reports durable storage and non-empty compression metadata for each record.
4. Query for wording related to one record and verify that record ranks above the unrelated record.
5. Close CrowClaw completely, relaunch it, repeat the query, and verify the same stored record is returned.
6. Verify the flow uses no Python process, network service, or external CrowQuant checkout.
7. Uninstall and verify the documented application-data retention behavior remains unchanged.

No Alpha 2 release may claim this test passes until it has been executed against the packaged installer and its exact evidence has replaced the pending fields in the Alpha 2 release notes.
