# CrowClaw operating contract

CrowClaw is Crow's original creation. This repository is the canonical public,
non-developer CrowClaw-Desktop edition: an installable Windows desktop AI-agent
application and the only product in this repository.

## Hard boundaries

- Do not edit, import, rewrite, delete, merge, cherry-pick, or repurpose files or history from any existing SRH-HQRE, Orion, CrowMemory, CrowNest, CrowQuant, or CrowClaw repository.
- Do not redefine CrowClaw as SRH-HQRE, an Orion public interface, a research website, an explainer, a status dashboard, or a wrapper around another AI chat.
- Do not expose Crow's personal accounts, corpus locations, credentials, machine-local paths, or private research in the product or public release.
- Use no paid service as a prerequisite. Local model operation is the default.
- Ask Crow instead of inventing identity, branding, scientific, licensing, account, or publication decisions.

## Worktree discipline

- Use the active checkout and assign explicit paths to concurrent writers. Create an isolated worktree only when overlapping work or the requested task requires one.
- A component agent edits only its owned paths.
- `main` is integration-only. Merge verified component commits; do not develop features directly on `main`.
- Serialize integration and avoid overlapping edits.
- Check changed behavior with relevant existing tests. Use full fresh-checkout acceptance for a release or a packaging/portability change, not every small edit.

## Release truth

- Report exact passing and failing gates.
- Do not call a status screen, mock, disabled control, source archive, or untested installer a usable Alpha.
- A public release is allowed only when the acceptance workflow in `docs/PRODUCT-CONTRACT.md` passes or every remaining failure is stated plainly in the release notes.
