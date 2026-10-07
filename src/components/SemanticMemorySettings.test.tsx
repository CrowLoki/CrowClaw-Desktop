import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SemanticMemorySettings } from "./SemanticMemorySettings";
import type { MemorySettings } from "../gateway/contracts";

describe("Local semantic profile controls", () => {
  it("saves the explicitly chosen local model while preserving source indexing choices", async () => {
    const configure = vi.fn().mockResolvedValue(undefined);
    const settings: MemorySettings = { indexConversations: false, indexActions: true };
    const user = userEvent.setup();
    render(<SemanticMemorySettings settings={settings} busy={false} onConfigure={configure} onIndex={vi.fn()} />);
    expect(configure).not.toHaveBeenCalled();
    await user.click(screen.getByLabelText("Enable a local embedding profile"));
    expect(screen.getByText(/Text from the indexed sources you enable/)).toBeVisible();
    await user.type(screen.getByLabelText("Embedding model identifier"), "my-installed-embedding-model");
    await user.clear(screen.getByLabelText("Model output dimensions"));
    await user.type(screen.getByLabelText("Model output dimensions"), "1024");
    await user.click(screen.getByRole("button", { name: "Save semantic profile" }));
    await waitFor(() => expect(configure).toHaveBeenCalledWith({ ...settings, embedding: { provider: "ollama", baseUrl: "http://127.0.0.1:11434", model: "my-installed-embedding-model", dimensions: 1024 } }));
  });

  it("allows immediate disabling while an embedding operation is busy", async () => {
    const configure = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    const settings: MemorySettings = { indexConversations: true, indexActions: false, embedding: { provider: "openai", baseUrl: "http://127.0.0.1:1234/v1", model: "local-model", dimensions: 768 } };
    render(<SemanticMemorySettings settings={settings} busy={true} onConfigure={configure} onIndex={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Save semantic profile" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Disable semantic retrieval now" }));
    expect(configure).toHaveBeenCalledWith({ ...settings, embedding: null });
  });

  it("does not reset an unsaved draft when only the index counts refresh", async () => {
    const user = userEvent.setup();
    const settings: MemorySettings = { indexConversations: true, indexActions: false, embedding: { provider: "ollama", baseUrl: "http://localhost:11434", model: "original", dimensions: 768 } };
    const props = { busy: false, onConfigure: vi.fn(), onIndex: vi.fn() };
    const { rerender } = render(<SemanticMemorySettings settings={settings} {...props} />);
    await user.clear(screen.getByLabelText("Embedding model identifier"));
    await user.type(screen.getByLabelText("Embedding model identifier"), "edited-but-not-saved");
    rerender(<SemanticMemorySettings settings={JSON.parse(JSON.stringify(settings))} status={{ state: "indexed", profileId: "profile", vectors: 20, pending: 0, detail: null }} {...props} />);
    expect(screen.getByLabelText("Embedding model identifier")).toHaveValue("edited-but-not-saved");
    expect(screen.getByText(/20 semantic vectors stored/)).toBeVisible();
  });
});
