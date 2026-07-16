import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createTauriGateway, TAURI_COMMANDS } from "./tauriGateway";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

describe("Tauri command contract", () => {
  beforeEach(() => invokeMock.mockReset());

  it("uses explicit, unique CrowClaw command names", () => {
    const commands = Object.values(TAURI_COMMANDS);
    expect(new Set(commands).size).toBe(commands.length);
    expect(commands.every((command) => command.startsWith("crowclaw_"))).toBe(true);
    expect(TAURI_COMMANDS.decideAction).toBe("crowclaw_action_decide");
    expect(TAURI_COMMANDS.cancelTask).toBe("crowclaw_task_cancel");
    expect(TAURI_COMMANDS.listCrowQuantMemories).toBe("crowclaw_crowquant_list");
    expect(TAURI_COMMANDS.rememberCrowQuant).toBe("crowclaw_crowquant_remember");
    expect(TAURI_COMMANDS.recallCrowQuant).toBe("crowclaw_crowquant_recall");
  });

  it("sends CrowQuant requests in the native command envelope", async () => {
    const gateway = createTauriGateway();
    invokeMock.mockResolvedValue(undefined);

    await gateway.listCrowQuantMemories();
    await gateway.rememberCrowQuant("Remember this");
    await gateway.recallCrowQuant("this", 8);

    expect(invokeMock).toHaveBeenNthCalledWith(1, "crowclaw_crowquant_list", undefined);
    expect(invokeMock).toHaveBeenNthCalledWith(2, "crowclaw_crowquant_remember", {
      request: { text: "Remember this" },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "crowclaw_crowquant_recall", {
      request: { query: "this", limit: 8 },
    });
  });
});

