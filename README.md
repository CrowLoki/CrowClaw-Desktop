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

## Current public Alpha

[CrowClaw v0.1.0-alpha.2](https://github.com/CrowLoki/CrowClaw-Desktop/releases/tag/v0.1.0-alpha.2) is the current published public prerelease. It adds a real, local CrowQuant-compatible memory path while preserving the Alpha 1 desktop-agent baseline.

- remember user-entered text from CrowClaw's Memory surface;
- compress deterministic local vectors into durable CrowQuant-compatible blocks;
- retrieve and rank relevant stored records from a local query;
- retain the stored text, compressed vector data, and retrieval behavior after CrowClaw is closed and reopened;
- require no Python installation, paid service, remote account, or separate repository at runtime.

The exact Alpha 2 release evidence and known limitations are recorded in [`docs/release-notes/v0.1.0-alpha.2.md`](docs/release-notes/v0.1.0-alpha.2.md). The published Windows installer has SHA-256 `4193872c1b6169f53c322906504f09a999c8a456f3e35ef5e26e5c1e020e8f9e`.

## Alpha 3 scope

Alpha 3 has completed local packaged acceptance and is being finalized as the next public prerelease. Its scope is deliberately bounded:

- let the conversational agent propose CrowQuant remember and search actions through the existing native approval gate;
- make denial non-mutating and keep approved CrowQuant results visible to the connected model;
- preserve stored CrowQuant memories and ranked recall across a full application restart;
- apply CrowClaw's supplied crow branding and a black, violet, purple, and magenta desktop theme while preserving the working desktop flows.

The integrated CrowQuant chat tools and violet-branded interface have passed a real installed-app acceptance run, including denial, approval, persistence, ranked recall, file approval, cancellation, settings, restart, and uninstall retention. Publication remains separate until the exact tag workflow passes and binds the public artifacts. See [`docs/release-notes/v0.1.0-alpha.3.md`](docs/release-notes/v0.1.0-alpha.3.md).

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
