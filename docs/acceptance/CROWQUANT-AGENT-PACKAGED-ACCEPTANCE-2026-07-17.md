# CrowQuant agent packaged acceptance — 2026-07-17

## Scope

This receipt covers the approval-gated `remember_memory` and `search_memory` agent paths in a real installed CrowClaw Windows application. It does not declare a public release.

- Integration commit: `12e1aa425e67589cb5c19d9c736e8ac1e39fd732`
- Installed NSIS test artifact SHA-256: `bc169e77a71f7eb4ea3a3f4a0a56efc4e73b44aff8fa08678f2218cafb989031`
- Installed NSIS test artifact size: `4,026,414` bytes
- Model endpoint: isolated OpenAI-compatible loopback test server
- Model: `crowclaw-acceptance-model`
- Initial CrowQuant memory count: `0`

The test artifact still carried the Alpha 2 package version while exercising post-Alpha-2 integration code. It is not a release asset and must be rebuilt under the next release version after all intended changes are integrated.

## Installed-app sequence and evidence

1. **Denied remember**
   - Prompt: `MEMORY DENY`
   - Proposed text: `This denied sentinel must never be stored`
   - The native approval dialog exposed the exact text and local CrowQuant write.
   - Denial response: `You denied storing that CrowQuant memory. Nothing was stored.`
   - CrowQuant memory count remained `0`.
   - The denied action stored no result payload.

2. **Approved quantum remember**
   - Prompt: `MEMORY QUANTUM`
   - Stored ID: `agent-action-63c3403f-074a-40c1-90bc-e6a2408aa950`
   - Stored text: `Superconducting qubit calibration preserves phase coherence`
   - Original bytes: `2048`
   - Compressed bytes: `161`
   - Algorithm: `CrowQuant WHT + Lloyd-Max 4-bit (native)`
   - The connected model received and repeated the actual ID, text, byte counts, and algorithm returned by the executed tool.

3. **Approved grocery remember**
   - Prompt: `MEMORY GROCERY`
   - Stored ID: `agent-action-44028902-c501-4afc-84a7-bbcae48da047`
   - Stored text: `Grocery list with apples bread and milk`
   - Original bytes: `2048`
   - Compressed bytes: `161`
   - Algorithm: `CrowQuant WHT + Lloyd-Max 4-bit (native)`
   - CrowQuant memory count became exactly `2`.

4. **Close and reopen persistence**
   - The installed CrowClaw window was closed and a distinct application window was launched.
   - The conversation reopened without first-run onboarding.
   - Both CrowQuant records and their IDs survived the restart.

5. **Denied search**
   - Prompt: `SEARCH DENY`
   - Proposed query: `qubit calibration`
   - Proposed limit: `2`
   - Denial response: `You denied searching CrowQuant memory. No stored memory was read.`
   - The denied action stored no result payload.
   - Memory count remained `2` and the record fingerprint remained unchanged.

6. **Approved ranked search**
   - Prompt: `SEARCH QUANTUM`
   - Query returned exactly `2` results.
   - Rank 1: quantum memory, score `0.5364276073999666`.
   - Rank 2: grocery memory, score `0.08716239282338994`.
   - The connected model received and repeated the actual query, result count, top ID, top text, and top score returned by the executed tool.
   - The memory count remained `2`.
   - The post-search record fingerprint matched the pre-search fingerprint: `27af06e48ff75645520a070c6a7d12ea6dc6ba10310193df762b562b0b3106a2`.

## Source and integration gates

- Rust library tests: `38/38`
- Agent runtime tests: `23/23`
- Storage tests: `7/7`
- Frontend tests: `9/9`
- Production frontend build: passed
- Strict all-target Clippy: passed with only the three documented pre-existing allowances
- Rust formatting check: passed
- Git diff check: passed

## Cleanup

- The acceptance database was isolated from Crow's normal CrowClaw data.
- Crow's normal app data was restored after the run.
- The loopback test model was stopped.
- The unpublished integration build was replaced with the exact published Alpha 2 installer, verified by SHA-256 `4193872c1b6169f53c322906504f09a999c8a456f3e35ef5e26e5c1e020e8f9e`.
- CrowClaw was left closed.
