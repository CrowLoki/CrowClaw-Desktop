import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { CrowQuantMemory } from "../gateway/contracts";
import { createDevelopmentGateway } from "../gateway/developmentGateway";
import { MemoryView } from "./MemoryView";

function savedNote(id: string, text: string): CrowQuantMemory {
  return { id, text, createdAt: "2026-10-08T00:00:00Z", originalBytes: 2048, compressedBytes: 161, compressionRatio: 12.72, algorithm: "CrowQuant" };
}

async function setup() {
  const gateway = createDevelopmentGateway({ delayMs: 0 });
  await gateway.configureMemory({ indexConversations: false, indexActions: false });
  let resolveSave!: (note: CrowQuantMemory) => void;
  let rejectSave!: (cause: Error) => void;
  const pending = new Promise<CrowQuantMemory>((resolve, reject) => { resolveSave = resolve; rejectSave = reject; });
  const remember = vi.fn().mockReturnValueOnce(pending).mockResolvedValueOnce(savedNote("second", "second draft"));
  render(<MemoryView gateway={gateway} memories={[]} listCrowQuantMemories={gateway.listCrowQuantMemories} rememberCrowQuant={remember} recallCrowQuant={gateway.recallCrowQuant} />);
  const input = screen.getByRole("textbox", { name: "Remember something" });
  const user = userEvent.setup();
  fireEvent.change(input, { target: { value: "  first draft  " } });
  await user.click(screen.getByRole("button", { name: "Remember with CrowQuant" }));
  expect(remember).toHaveBeenCalledWith("first draft");
  return { user, input, remember, resolveSave, rejectSave };
}

describe("CrowQuant note submission", () => {
  it("preserves a newer draft when an earlier save completes", async () => {
    const { user, input, remember, resolveSave } = await setup();
    fireEvent.change(input, { target: { value: "second draft" } });
    await act(async () => resolveSave(savedNote("first", "first draft")));
    expect(input).toHaveValue("second draft");
    expect(screen.getByRole("button", { name: "Remember with CrowQuant" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Remember with CrowQuant" }));
    await waitFor(() => expect(remember).toHaveBeenNthCalledWith(2, "second draft"));
    await waitFor(() => expect(input).toHaveValue(""));
  });

  it("clears an unchanged draft only after its successful save", async () => {
    const { input, resolveSave } = await setup();
    expect(input).toHaveValue("  first draft  ");
    await act(async () => resolveSave(savedNote("first", "first draft")));
    expect(input).toHaveValue("");
    expect(screen.getByText("Stored locally with CrowQuant.")).toBeVisible();
  });

  it("retains the newer draft and reports a failed save", async () => {
    const { input, rejectSave } = await setup();
    fireEvent.change(input, { target: { value: "second draft" } });
    await act(async () => rejectSave(new Error("Storage unavailable")));
    expect(input).toHaveValue("second draft");
    expect(screen.getByRole("alert")).toHaveTextContent("Storage unavailable");
    expect(screen.queryByText("Stored locally with CrowQuant.")).not.toBeInTheDocument();
  });
});
