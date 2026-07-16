import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { createDevelopmentGateway } from "./gateway/developmentGateway";

describe("CrowClaw desktop shell", () => {
  it("completes first-run local model onboarding", async () => {
    const user = userEvent.setup();
    render(<App gateway={createDevelopmentGateway({ firstRun: true, delayMs: 0 })} />);

    expect(await screen.findByRole("heading", { name: /your local agent/i })).toBeVisible();
    await user.click(screen.getByText("Ollama", { selector: "strong" }));
    await user.click(screen.getByRole("button", { name: /test connection/i }));
    expect(await screen.findByText(/connected to ollama/i)).toBeVisible();
    await user.click(screen.getByRole("button", { name: /connect and open crowclaw/i }));

    expect(await screen.findByRole("navigation", { name: /crowclaw sections/i })).toBeVisible();
    expect(screen.getByRole("textbox", { name: /message crowclaw/i })).toBeEnabled();
    expect(screen.getByText("Ollama", { selector: ".model-chip" })).toBeVisible();
  });

  it("shows a proposed file action and records a denial before anything runs", async () => {
    const user = userEvent.setup();
    render(<App gateway={createDevelopmentGateway({ firstRun: false, delayMs: 0 })} />);

    const composer = await screen.findByRole("textbox", { name: /message crowclaw/i });
    await user.type(composer, "Inspect a folder");
    await user.click(screen.getByRole("button", { name: /send message/i }));

    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByRole("heading", { name: /read text files/i })).toBeVisible();
    expect(within(dialog).getByText("User-selected folder · *.txt")).toBeVisible();
    expect(within(dialog).getByText(/nothing in this request runs unless you approve/i)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: /^deny$/i }));

    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(screen.getByText(/you denied the file read/i, { selector: ".message__body p" })).toBeVisible();
  });

  it("shows and cancels a running task", async () => {
    const user = userEvent.setup();
    render(
      <App
        gateway={createDevelopmentGateway({
          firstRun: false,
          includeRunningTask: true,
          delayMs: 0,
        })}
      />,
    );

    await user.click(await screen.findByRole("button", { name: /tasks/i }));
    expect(screen.getByRole("heading", { name: "Review selected notes" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: /^cancel$/i }));

    expect(await screen.findByText("Cancelled", { selector: ".status-label" })).toBeVisible();
    expect(screen.getByText("Cancelled by you")).toBeVisible();
  });

  it("changes and saves permission settings", async () => {
    const user = userEvent.setup();
    render(<App gateway={createDevelopmentGateway({ firstRun: false, delayMs: 0 })} />);

    await user.click(await screen.findByRole("button", { name: /settings/i }));
    const filePermission = screen.getByRole("combobox", { name: /read local files/i });
    await user.selectOptions(filePermission, "deny");
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    expect(await screen.findByText("Saved", { selector: ".section-stat" })).toBeVisible();
    expect(filePermission).toHaveValue("deny");
  });
});
