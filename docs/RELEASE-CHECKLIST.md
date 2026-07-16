# Windows Alpha release checklist

## Source and component gates

- [ ] Every component commit is pushed from its dedicated branch/worktree.
- [ ] `main` contains only reviewed component merges and integration wiring.
- [ ] Fresh clone has no dependency on another local repository or Crow-specific path.
- [ ] `npm ci`, frontend tests, frontend build, `cargo test --locked`, and Tauri build pass.
- [ ] Secret and private-source scan passes.

## Product acceptance

- [ ] Install from the produced NSIS installer as a standard Windows user.
- [ ] Launch from Start menu.
- [ ] Complete local-model onboarding.
- [ ] Chat with the model.
- [ ] Request a folder inspection and verify no read happens before approval.
- [ ] Approve one text-file read and verify its real contents influence the response.
- [ ] Close and reopen CrowClaw and verify the conversation/action persists.
- [ ] Cancel a running task.
- [ ] Change connection and permission settings.
- [ ] Uninstall and verify the selected data-retention behavior.

## Publication

- [ ] Installer assets are hashed in `SHA256SUMS.txt`.
- [ ] `release-manifest.json` records the exact Git commit and asset hashes.
- [ ] Release notes state every known limitation, including signing status.
- [ ] The repository contains no SRH-HQRE private corpus, account credentials, or machine-local paths.
- [ ] Tag points at the exact verified commit before the release is created.
