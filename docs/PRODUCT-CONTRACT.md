# CrowClaw product contract

## Concrete product

CrowClaw is a Windows desktop AI-agent application. A person installs it, launches it normally, connects a local model, chats, authorises tool actions, gives it tasks, closes it, reopens it without losing the conversation, and can update, repair, or uninstall it.

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
