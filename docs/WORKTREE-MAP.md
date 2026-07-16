# CrowClaw component worktree map

Every component is developed in a separate branch and physical Git worktree.

| Component | Branch | Owned paths | Acceptance boundary |
| --- | --- | --- | --- |
| Desktop shell | `codex/desktop-shell` | `src/**`, frontend tests | Real chat/task/settings UI with no disabled placeholder navigation |
| Agent runtime | `codex/agent-runtime` | `src-tauri/src/agent/**`, `src-tauri/src/tools/**` | Local model request/response and approval-gated tool loop |
| Persistence | `codex/persistence` | `src-tauri/src/storage/**`, migrations and storage tests | Conversations, messages, actions, tasks, and settings survive restart |
| Windows release | `codex/windows-release` | installer configuration, release scripts, CI, packaging docs | Fresh-checkout build, installer, uninstall, checksums and honest release notes |
| Integration | `main` | shared wiring only | Merge verified component commits and pass packaged end-to-end acceptance |

Component worktrees live outside the repository checkout under `C:\Users\djdar\Documents\CrowClaw-Worktrees\`. They are disposable checkouts; branches and commits are the durable record.
