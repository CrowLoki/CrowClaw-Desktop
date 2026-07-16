# CrowClaw Desktop

CrowClaw is a local-first, installable Windows desktop AI agent. It is being built as a real application Crow can launch, talk to, give tasks to, close, reopen, update, repair, and uninstall without depending on a paid AI service.

The Alpha 1 baseline provides:

- a real desktop chat interface;
- connection to local OpenAI-compatible model servers, including LM Studio, Ollama, and llama.cpp;
- persistent conversations and settings;
- an agent loop with visible, user-approved tool actions;
- local file and command tools behind explicit approval;
- clean Windows installation and uninstallation;
- a fresh-checkout build and end-to-end acceptance test.

## Alpha 2 scope

Alpha 2 is scoped to add a real, local CrowQuant-compatible memory path while preserving the Alpha 1 desktop-agent baseline. Its release remains pending until the packaged-installer acceptance evidence is recorded.

- remember user-entered text from CrowClaw's Memory surface;
- compress deterministic local vectors into durable CrowQuant-compatible blocks;
- retrieve and rank relevant stored records from a local query;
- retain the stored text, compressed vector data, and retrieval behavior after CrowClaw is closed and reopened;
- require no Python installation, paid service, remote account, or separate repository at runtime.

The exact Alpha 2 release evidence and known limitations belong in [`docs/release-notes/v0.1.0-alpha.2.md`](docs/release-notes/v0.1.0-alpha.2.md). Until those pending fields are replaced, the Alpha 2 installer has not been declared accepted or published.

This repository is intentionally independent. It does not modify or repurpose the existing SRH-HQRE, Orion, CrowMemory, CrowNest, CrowQuant, or previous CrowClaw repositories.

## Development

Prerequisites: Node.js, npm, Rust, and the Windows requirements for Tauri 2.

```powershell
npm install
npm run tauri dev
```

Build the Windows installer:

```powershell
npm run tauri build
```

The authoritative product boundary and acceptance test are in [`docs/PRODUCT-CONTRACT.md`](docs/PRODUCT-CONTRACT.md).
