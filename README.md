# CrowClaw Desktop

CrowClaw is a local-first, installable Windows desktop AI agent. It is being built as a real application Crow can launch, talk to, give tasks to, close, reopen, update, repair, and uninstall without depending on a paid AI service.

The first Alpha is required to provide:

- a real desktop chat interface;
- connection to local OpenAI-compatible model servers, including LM Studio, Ollama, and llama.cpp;
- persistent conversations and settings;
- an agent loop with visible, user-approved tool actions;
- local file and command tools behind explicit approval;
- clean Windows installation and uninstallation;
- a fresh-checkout build and end-to-end acceptance test.

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
