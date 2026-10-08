import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ComposerModelSource, ConversationModelChoice } from "../gateway/composerContracts";
import { ComposerModelControls } from "./ComposerModelControls";

const sources: ComposerModelSource[] = [
  { id: "account", label: "Personal", provider: "Provider", status: "ready", models: [
    { id: "sol", displayName: "Sol", reasoningEfforts: ["low", "high", "ultra"] },
    { id: "luna", displayName: "Luna", reasoningEfforts: ["low", "high"] },
  ] },
  { id: "local", label: "Local", provider: "Ollama", status: "ready", models: [
    { id: "installed", displayName: "Installed model", reasoningEfforts: [] },
  ] },
  { id: "offline", label: "Offline account", provider: "Provider", status: "disconnected", models: [
    { id: "sol", displayName: "Sol", reasoningEfforts: ["ultra"] },
  ] },
];
const choice: ConversationModelChoice = { providerProfileId: "account", model: "sol", reasoningEffort: "high" };
const defaults = () => ({ value: choice, sources, onChoose: vi.fn().mockResolvedValue(undefined), onRefresh: vi.fn().mockResolvedValue(undefined) });
const apply = () => screen.getByRole("button", { name: "Use for next message" });
async function expand() {
  const user = userEvent.setup();
  await user.click(screen.getByText("Model & effort"));
  return user;
}
function deferred() {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("ComposerModelControls", () => {
  it("uses model-specific effort metadata and resets effort when explicitly changing model", async () => {
    const props = defaults();
    render(<ComposerModelControls {...props} />);
    const user = await expand();
    const effort = screen.getByLabelText("Reasoning effort");
    expect(within(effort).getByRole("option", { name: "ultra" })).toBeEnabled();
    await user.selectOptions(effort, "effort:ultra");
    expect(screen.getByText("Sol · high")).toBeVisible();
    expect(props.onChoose).not.toHaveBeenCalled();
    await user.selectOptions(screen.getByLabelText("Model"), "luna");
    expect(effort).toHaveValue("default");
    expect(within(effort).queryByRole("option", { name: "ultra" })).not.toBeInTheDocument();
    await user.click(apply());
    expect(props.onChoose).toHaveBeenCalledWith({ ...choice, model: "luna", reasoningEffort: null });
  });

  it("requires explicit source and model choices and invents no local effort metadata", async () => {
    const props = { ...defaults(), value: null };
    render(<ComposerModelControls {...props} />);
    const user = await expand();
    expect(apply()).toBeDisabled();
    await user.selectOptions(screen.getByLabelText("Provider/account"), "account");
    expect(screen.getByLabelText("Model")).toHaveValue("");
    expect(apply()).toBeDisabled();
    await user.selectOptions(screen.getByLabelText("Model"), "sol");
    expect(apply()).toBeEnabled();
    await user.selectOptions(screen.getByLabelText("Provider/account"), "local");
    expect(apply()).toBeDisabled();
    expect(screen.getByLabelText("Model")).toHaveValue("");
    await user.selectOptions(screen.getByLabelText("Model"), "installed");
    expect(within(screen.getByLabelText("Reasoning effort")).getAllByRole("option")).toHaveLength(1);
    await user.click(apply());
    expect(props.onChoose).toHaveBeenCalledWith({ providerProfileId: "local", model: "installed", reasoningEffort: null });
  });

  it.each([
    [{ ...choice, providerProfileId: "missing" }, /unavailable or disconnected/],
    [{ ...choice, providerProfileId: "offline" }, /unavailable or disconnected/],
    [{ ...choice, model: "removed" }, /model is no longer available/],
    [{ ...choice, model: "luna", reasoningEffort: "ultra" }, /effort is no longer supported/],
  ] as const)("blocks unavailable selections %#", async (value, message) => {
    const props = defaults();
    render(<ComposerModelControls {...props} value={value} />);
    await expand();
    expect(screen.getByRole("alert")).toHaveTextContent(message);
    expect(apply()).toBeDisabled();
    fireEvent.click(apply());
    expect(props.onChoose).not.toHaveBeenCalled();
  });

  it("preserves supported staged effort through refresh and blocks removed effort until corrected", async () => {
    const props = defaults();
    const { rerender } = render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.selectOptions(screen.getByLabelText("Reasoning effort"), "effort:ultra");
    await user.click(screen.getByRole("button", { name: "Refresh models" }));
    expect(props.onRefresh).toHaveBeenCalledWith("account");
    rerender(<ComposerModelControls {...props} sources={structuredClone(sources)} />);
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("effort:ultra");
    const updated = structuredClone(sources);
    updated[0].models[0].reasoningEfforts = ["high", "dynamic-effort"];
    rerender(<ComposerModelControls {...props} sources={updated} />);
    expect(apply()).toBeDisabled();
    expect(screen.getByLabelText("Reasoning effort")).toHaveAttribute("aria-invalid", "true");
    expect(screen.queryByRole("option", { name: "ultra" })).not.toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("Reasoning effort"), "effort:dynamic-effort");
    await user.click(apply());
    expect(props.onChoose).toHaveBeenCalledWith({ ...choice, reasoningEffort: "dynamic-effort" });
  });

  it("blocks a source that disconnects while open without selecting a different source", async () => {
    const props = defaults();
    const { rerender } = render(<ComposerModelControls {...props} />);
    await expand();
    const updated = structuredClone(sources);
    updated[0].status = "disconnected";
    rerender(<ComposerModelControls {...props} sources={updated} />);
    expect(screen.getByLabelText("Provider/account")).toHaveValue("account");
    expect(apply()).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refresh models" })).toBeEnabled();
    expect(screen.getByRole("option", { name: /Offline account/ })).toBeDisabled();
  });

  it("shows pending state, prevents duplicate operations, and retains staged choices after submit failure", async () => {
    const task = deferred();
    const props = { ...defaults(), onChoose: vi.fn().mockReturnValueOnce(task.promise).mockResolvedValue(undefined) };
    render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.selectOptions(screen.getByLabelText("Reasoning effort"), "effort:ultra");
    const button = apply();
    act(() => { fireEvent.click(button); fireEvent.click(button); });
    expect(props.onChoose).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("Applying selection");
    expect(screen.getByText("Sol · high")).toBeVisible();
    expect(screen.getByRole("button", { name: "Refresh models" })).toBeDisabled();
    expect(screen.getByLabelText("Model")).toBeDisabled();
    await act(async () => task.reject(new Error("Selection rejected")));
    expect(screen.getByRole("alert")).toHaveTextContent("Selection rejected");
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("effort:ultra");
    expect(apply()).toBeEnabled();
    await user.click(apply());
    expect(props.onChoose).toHaveBeenLastCalledWith({ ...choice, reasoningEffort: "ultra" });
    expect(screen.getByRole("status")).toHaveTextContent("Selection applied for the next message");
  });

  it("honors external busy state and displays external errors", async () => {
    const props = defaults();
    render(<ComposerModelControls {...props} busy error="Provider temporarily unavailable" />);
    await expand();
    expect(apply()).toBeDisabled();
    expect(screen.getByLabelText("Provider/account")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refresh models" })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("Provider temporarily unavailable");
    fireEvent.click(apply());
    expect(props.onChoose).not.toHaveBeenCalled();
  });

  it("makes refresh failures recoverable without discarding staged choices", async () => {
    const task = deferred();
    const props = { ...defaults(), onRefresh: vi.fn().mockReturnValueOnce(task.promise).mockResolvedValue(undefined) };
    render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.selectOptions(screen.getByLabelText("Reasoning effort"), "effort:ultra");
    await user.click(screen.getByRole("button", { name: "Refresh models" }));
    expect(apply()).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Refreshing models");
    await act(async () => task.reject(new Error("Refresh unavailable")));
    expect(screen.getByRole("alert")).toHaveTextContent("Refresh unavailable");
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("effort:ultra");
    await user.click(screen.getByRole("button", { name: "Refresh models" }));
    expect(props.onRefresh).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("synchronizes changed parent values, preserves drafts on equivalent values, and clears on null", async () => {
    const props = defaults();
    const { rerender } = render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.selectOptions(screen.getByLabelText("Reasoning effort"), "effort:ultra");
    rerender(<ComposerModelControls {...props} value={{ ...choice }} />);
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("effort:ultra");
    rerender(<ComposerModelControls {...props} value={{ ...choice, model: "luna", reasoningEffort: null }} />);
    expect(screen.getByLabelText("Model")).toHaveValue("luna");
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("default");
    expect(screen.getByText("Luna · Provider default")).toBeVisible();
    rerender(<ComposerModelControls {...props} value={null} />);
    expect(screen.getByLabelText("Provider/account")).toHaveValue("");
    expect(apply()).toBeDisabled();
  });

  it("does not overwrite a newer parent selection when an older request finishes", async () => {
    const task = deferred();
    const props = { ...defaults(), onChoose: vi.fn().mockReturnValue(task.promise) };
    const { rerender } = render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.click(apply());
    rerender(<ComposerModelControls {...props} value={{ ...choice, model: "luna", reasoningEffort: null }} />);
    await act(async () => task.reject(new Error("Old request failed")));
    expect(screen.getByLabelText("Model")).toHaveValue("luna");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(apply()).toBeEnabled();
  });

  it("provides a keyboard-focusable native disclosure and labels without submitting an enclosing composer", async () => {
    const submit = vi.fn(event => event.preventDefault());
    render(<form onSubmit={submit}><ComposerModelControls {...defaults()} /></form>);
    const user = userEvent.setup();
    await user.tab();
    expect(screen.getByText("Model & effort").closest("summary")).toHaveFocus();
    await user.click(screen.getByText("Model & effort"));
    expect(screen.getByText(/Applies to the next message, not running tasks\./)).toBeVisible();
    expect(screen.getByText(/selected provider receives this conversation's message history/)).toBeVisible();
    screen.getByLabelText("Provider/account").focus();
    await user.tab();
    expect(screen.getByLabelText("Model")).toHaveFocus();
    await user.click(apply());
    expect(submit).not.toHaveBeenCalled();
  });
});
