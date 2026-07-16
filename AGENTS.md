# CrowClaw operating contract

CrowClaw is the only product in this repository: an installable Windows desktop AI-agent application.

## Hard boundaries

- Do not edit, import, rewrite, delete, merge, cherry-pick, or repurpose files or history from any existing SRH-HQRE, Orion, CrowMemory, CrowNest, CrowQuant, or CrowClaw repository.
- Do not redefine CrowClaw as SRH-HQRE, an Orion public interface, a research website, an explainer, a status dashboard, or a wrapper around another AI chat.
- Do not expose Crow's personal accounts, corpus locations, credentials, machine-local paths, or private research in the product or public release.
- Use no paid service as a prerequisite. Local model operation is the default.
- Ask Crow instead of inventing identity, branding, scientific, licensing, account, or publication decisions.

## Worktree discipline

- Each independently edited component has its own branch and Git worktree.
- A component agent edits only its owned paths.
- `main` is integration-only. Merge verified component commits; do not develop features directly on `main`.
- Do not share a writable checkout between agents.
- Build and test each component before integration, then run the full acceptance test from a fresh checkout.

## Release truth

- Report exact passing and failing gates.
- Do not call a status screen, mock, disabled control, source archive, or untested installer a usable Alpha.
- A public release is allowed only when the acceptance workflow in `docs/PRODUCT-CONTRACT.md` passes or every remaining failure is stated plainly in the release notes.
