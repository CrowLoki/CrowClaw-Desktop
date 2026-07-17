# CrowClaw Alpha 3 packaged acceptance — 2026-07-17

## Scope

This receipt records a native Windows acceptance run of the standalone CrowClaw Alpha 3 release candidate with CrowQuant agent tools and the violet CrowClaw interface integrated together. It records the locally exercised candidate; the public tag workflow remains responsible for rebuilding the exact tagged commit and binding the public installer hash.

- Product candidate commit: `cf70e50effdd3eff24744acaa2fec1078eb51360`
- Installed NSIS candidate SHA-256: `c12d40c515d96896d7a16ff799c36d6518e53631274b26003d8c54e5fa00c23e`
- Installed NSIS candidate size: `4,180,429` bytes
- Installed package version: `0.1.0-alpha.3`
- Model endpoint: isolated OpenAI-compatible loopback test server
- Model: `crowclaw-acceptance-model`

The candidate was built from a clean public-safe history whose parent is the published Alpha 2 commit. The Alpha 3 tree and reachable Alpha 3 history were scanned for machine-local paths, private corpus locators, account secrets, tokens, and private keys before this run.

## Native installed-app sequence and evidence

1. **Install and onboarding**
   - The exact candidate installer hash was verified immediately before installation.
   - The Windows uninstall registry reported version `0.1.0-alpha.3`.
   - First-run onboarding completed against the isolated loopback model.
   - The supplied crow emblem and black, violet, purple, and magenta visual system were visible in onboarding and the main workspace.

2. **Denied CrowQuant remember**
   - Prompt: `MEMORY DENY`
   - Proposed text: `This denied sentinel must never be stored`
   - The approval surface exposed the exact text, local boundary, action type, and risk before execution.
   - Denial response: `You denied storing that CrowQuant memory. Nothing was stored.`
   - No CrowQuant memory was written.

3. **Approved CrowQuant remembers**
   - Quantum ID: `agent-action-09a96f1b-e7cc-46bd-89f1-6ae3a9674aec`
   - Quantum text: `Superconducting qubit calibration preserves phase coherence`
   - Grocery ID: `agent-action-6d9bc9de-b9c9-41ef-8278-9416f4e658c2`
   - Grocery text: `Grocery list with apples bread and milk`
   - Each record compressed `2048` original bytes to `161` bytes with `CrowQuant WHT + Lloyd-Max 4-bit (native)`.
   - The Memory surface showed exactly `2 stored`, both records, and a `12.72x` ratio for each.

4. **Restart persistence**
   - CrowClaw was closed and launched again twice during the run.
   - The configured connection, conversation, both CrowQuant memories, both IDs, approved activity, file-derived response, and cancelled-task result survived restart.
   - After the final restart the Memory surface still showed `2 stored`, plus `5` approved activity records.

5. **Denied and approved CrowQuant search**
   - Denied prompt: `SEARCH DENY`
   - Proposed query: `qubit calibration`, limit `2`.
   - Denial response: `You denied searching CrowQuant memory. No stored memory was read.`
   - Approved prompt: `SEARCH QUANTUM`
   - The approved search returned exactly `2` records.
   - The quantum record ranked first with score `0.5364276073999666`; the grocery record ranked second.
   - The connected model received and repeated the actual top ID, text, count, and score returned by CrowQuant.

6. **Approval-gated folder and file access**
   - A test folder containing two unrelated text fixtures was selected through the native folder picker.
   - The first approval exposed only the exact directory-list action; no file content was read before approval.
   - After directory-list approval, a separate approval exposed the exact `garden-note.txt` read.
   - Only that read was approved.
   - The connected model then reported the real approved content: `The unrelated garden note says to water the basil before sunrise on Saturday.`

7. **Settings and cancellation**
   - The local-file permission was changed to `Always deny` and the Settings surface confirmed `Saved`.
   - A deliberately delayed model task was cancelled while running.
   - The Tasks surface recorded `CANCEL FAST` as `Cancelled`, and the conversation recorded that unexecuted actions were closed.
   - The normal acceptance connection was restored after the cancellation check.

8. **Runtime and uninstall retention**
   - The installed process tree contained the native CrowClaw executable and its WebView runtime; it did not require Python or a separate CrowQuant process.
   - CrowClaw was closed before uninstall.
   - Silent uninstall removed the installed application binary.
   - The isolated SQLite database, shared-memory file, and write-ahead log remained byte-for-byte unchanged across uninstall.
   - The isolated acceptance data was preserved separately, Crow's normal CrowClaw data was restored, the loopback model was stopped, and CrowClaw was left closed.

## Build and source gates

- Release-script tests: passed
- Frontend tests: `9/9`
- Rust library tests: `38/38`
- Agent runtime tests: `23/23`
- Storage tests: `7/7`
- Production frontend build: passed
- Tauri NSIS package build: passed
- Manifest version, commit, installer size, and installer hash verification: passed
- Reachable-history and tracked-tree public hygiene scan: passed
- Violet interface follow-up contrast review: passed

## Evidence boundary

This local receipt does not claim that the workflow-produced public installer has the same hash as the locally installed candidate. Windows packaging is rebuilt by the tag workflow. The public release notes, `SHA256SUMS.txt`, and `release-manifest.json` bind the workflow-produced installer to the exact tagged commit after that workflow passes.

## Continuing Alpha limitations

- CrowQuant retrieval is deterministic lexical retrieval, not a neural embedding model.
- Model-backed chat still requires a compatible endpoint; CrowQuant storage and retrieval remain local.
- The prerelease is not signed with a production Windows code-signing certificate.
- Updates remain manual.
