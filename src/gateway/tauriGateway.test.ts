import { describe, expect, it } from "vitest";
import { TAURI_COMMANDS } from "./tauriGateway";

describe("Tauri command contract", () => {
  it("uses explicit, unique CrowClaw command names", () => {
    const commands = Object.values(TAURI_COMMANDS);
    expect(new Set(commands).size).toBe(commands.length);
    expect(commands.every((command) => command.startsWith("crowclaw_"))).toBe(true);
    expect(TAURI_COMMANDS.decideAction).toBe("crowclaw_action_decide");
    expect(TAURI_COMMANDS.cancelTask).toBe("crowclaw_task_cancel");
  });
});

