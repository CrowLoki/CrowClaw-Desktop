import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AppSettings } from "../gateway/contracts";
import { SettingsView } from "./SettingsView";

const settings: AppSettings = {
  permissions: { readFiles: "ask", writeFiles: "deny", runCommands: "ask" },
  launchAtLogin: false, keepRunningOnClose: false, retainConversations: true, theme: "dark",
  personalities: [{ id: "crowbot-ai", name: "CrowBot AI", instruction: "A synthetic test voice." }],
  selectedPersonality: null,
};

describe("Local personality selection", () => {
  it("saves the chosen personality without changing permissions or other settings", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(<SettingsView settings={settings} onSave={onSave} />);
    await user.selectOptions(screen.getByLabelText("Selected personality"), "crowbot-ai");
    expect(onSave).not.toHaveBeenCalled();
    await user.click(screen.getByText("Personality instructions"));
    expect(screen.getByText("A synthetic test voice.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledWith({ ...settings, selectedPersonality: "crowbot-ai" }));
  });

  it("restores standard personality and supports older settings without profile fields", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    const { rerender } = render(<SettingsView settings={{ ...settings, selectedPersonality: "crowbot-ai" }} onSave={onSave} />);
    await user.selectOptions(screen.getByLabelText("Selected personality"), "");
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledWith(settings));
    const { personalities: _profiles, selectedPersonality: _selection, ...legacy } = settings;
    rerender(<SettingsView settings={legacy} onSave={onSave} />);
    expect(screen.getByLabelText("Selected personality")).toHaveValue("");
  });
});
