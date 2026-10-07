import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { createDevelopmentGateway } from "../gateway/developmentGateway";
import { NativeMemoryPanel } from "./NativeMemoryPanel";

describe("Native memory controls", () => {
  it("requires the upgrade choice before enabling past conversation indexing", async () => {
    const gateway = createDevelopmentGateway({ delayMs: 0 });
    await gateway.configureMemory({ indexConversations: null, indexActions: false });
    const configure = vi.spyOn(gateway, "configureMemory");
    const user = userEvent.setup();
    render(<NativeMemoryPanel gateway={gateway} />);
    await screen.findByRole("button", { name: "Index my CrowClaw conversations" });
    expect(configure).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Keep conversation indexing off" }));
    await waitFor(() => expect(configure).toHaveBeenCalledWith({ indexConversations: false, indexActions: false }));
  });

  it("searches by typed source and method, labels authorship, and withdraws only from recall", async () => {
    const gateway = createDevelopmentGateway({ delayMs: 0 });
    const record = await gateway.rememberCrowQuant("violet telescope calibration");
    const search = vi.spyOn(gateway, "searchMemory");
    const withdraw = vi.spyOn(gateway, "withdrawMemory");
    const user = userEvent.setup();
    render(<NativeMemoryPanel gateway={gateway} />);
    await screen.findByRole("heading", { name: "Search retained context" });
    await user.type(screen.getByLabelText("Search previous context"), "telescope");
    await user.selectOptions(screen.getByLabelText("Source"), "user_note");
    await user.selectOptions(screen.getByLabelText("Search method"), "full_text");
    await user.click(screen.getByRole("button", { name: "Search context" }));
    expect(await screen.findByText("violet telescope calibration")).toBeVisible();
    expect(screen.getByText("Your note · user")).toBeVisible();
    expect(search).toHaveBeenCalledWith({ query: "telescope", limit: 10, sourceKind: "user_note", mode: "full_text" });
    await user.click(screen.getByRole("button", { name: "Forget from search" }));
    await waitFor(() => expect(withdraw).toHaveBeenCalledWith(record.id));
    expect(await screen.findByText(/original conversation or note is retained/i)).toBeVisible();
    expect(await gateway.listCrowQuantMemories()).toHaveLength(1);
    await gateway.rebuildMemory();
    expect((await gateway.searchMemory({ query: "telescope", limit: 10, sourceKind: null, mode: "hybrid" })).hits).toHaveLength(0);
  });

  it("reports an indexing failure without erasing the old result", async () => {
    const gateway = createDevelopmentGateway({ delayMs: 0 });
    vi.spyOn(gateway, "rebuildMemory").mockRejectedValue(new Error("Index is busy"));
    const user = userEvent.setup();
    render(<NativeMemoryPanel gateway={gateway} />);
    await screen.findByRole("heading", { name: "Search retained context" });
    await user.click(screen.getByRole("button", { name: "Rebuild search index" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Index is busy");
  });
});
