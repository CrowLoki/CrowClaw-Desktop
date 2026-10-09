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
  await user.click(screen.getByText("Selection details"));
  return user;
}
function deferred() {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

// jsdom does not implement the browser's top layer. Exercise the component's
// capability checks and native-event seam without claiming WebView acceptance.
function nativePopoverFixture(invoker: boolean) {
  const changed: Array<[object, string, PropertyDescriptor | undefined]> = [];
  function define(target: object, key: string, value: unknown) {
    changed.push([target, key, Object.getOwnPropertyDescriptor(target, key)]);
    Object.defineProperty(target, key, { configurable: true, value });
  }
  function toggle(element: HTMLElement, state: "open" | "closed") {
    element.dataset.testPopoverState = state;
    const event = new Event("beforetoggle");
    Object.defineProperty(event, "newState", { value: state });
    element.dispatchEvent(event);
  }
  const show = vi.fn(function (this: HTMLElement) { toggle(this, "open"); });
  const hide = vi.fn(function (this: HTMLElement) { toggle(this, "closed"); });
  define(HTMLElement.prototype, "popover", "auto");
  define(HTMLElement.prototype, "showPopover", show);
  define(HTMLElement.prototype, "hidePopover", hide);
  if (invoker) define(HTMLButtonElement.prototype, "commandForElement", null);
  const originalMatches = Element.prototype.matches;
  const matches = vi.spyOn(Element.prototype, "matches").mockImplementation(function (this: HTMLElement, selector) {
    return selector === ":popover-open" ? this.dataset.testPopoverState === "open" : originalMatches.call(this, selector);
  });
  return { show, hide, toggle, restore: () => {
    matches.mockRestore();
    for (const [target, key, descriptor] of changed.reverse()) {
      if (descriptor) Object.defineProperty(target, key, descriptor);
      else Reflect.deleteProperty(target, key);
    }
  } };
}

describe("ComposerModelControls", () => {
  it("lists connected models in provider groups, searches by provider or model, and stages a button choice", async () => {
    const props = defaults();
    render(<ComposerModelControls {...props} />);
    const user = await expand();
    const personal = screen.getByRole("group", { name: "Personal · Provider" });
    expect(within(personal).getByRole("button", { name: /Sol.*Selected/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("group", { name: "Local · Ollama" })).toBeVisible();
    expect(within(screen.getByRole("group", { name: /Offline account · Provider/ })).getByRole("button")).toBeDisabled();
    await user.type(screen.getByLabelText("Search models"), "luna");
    expect(within(personal).queryByRole("button", { name: /^Sol/ })).not.toBeInTheDocument();
    await user.click(within(personal).getByRole("button", { name: /^Luna/ }));
    expect(props.onChoose).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("default");
    await user.click(apply());
    expect(props.onChoose).toHaveBeenCalledWith({ ...choice, model: "luna", reasoningEffort: null });
    await user.clear(screen.getByLabelText("Search models"));
    await user.type(screen.getByLabelText("Search models"), "OLLAMA");
    expect(screen.queryByRole("group", { name: "Personal · Provider" })).not.toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Local · Ollama" })).toBeVisible();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("changes only visibility, keeps a hidden selected model available, and lists every model in edit view", async () => {
    const props = { ...defaults(), onHiddenModelsChange: vi.fn().mockResolvedValue(undefined) };
    const original = structuredClone(sources);
    render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.selectOptions(screen.getByLabelText("Reasoning effort"), "effort:ultra");
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    expect(screen.getAllByRole("switch")).toHaveLength(4);
    await user.click(screen.getByRole("switch", { name: "Show Sol from Personal" }));
    expect(props.onHiddenModelsChange).toHaveBeenLastCalledWith(['["account","sol"]']);
    await user.click(screen.getByRole("switch", { name: "Show Luna from Personal" }));
    expect(props.onHiddenModelsChange).toHaveBeenLastCalledWith(['["account","sol"]', '["account","luna"]']);
    await user.click(screen.getByRole("button", { name: "Back to picker" }));
    const personal = screen.getByRole("group", { name: "Personal · Provider" });
    expect(within(personal).getByRole("button", { name: /Sol.*Selected.*Hidden from list/ })).toBeEnabled();
    expect(within(personal).queryByRole("button", { name: /^Luna/ })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("effort:ultra");
    expect(screen.getByText("Sol · high")).toBeVisible();
    expect(props.onChoose).not.toHaveBeenCalled();
    expect(props.onRefresh).not.toHaveBeenCalled();
    expect(sources).toEqual(original);
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    expect(screen.getByRole("switch", { name: "Show Luna from Personal" })).not.toBeChecked();
    expect(screen.getAllByRole("switch")).toHaveLength(4);
  });

  it("uses collision-safe provider/model keys even with equal model IDs and delimiter-like identifiers", async () => {
    const catalog: ComposerModelSource[] = [
      { ...sources[0], id: 'one",two', models: [{ id: "same", displayName: "First model", reasoningEfforts: [] }] },
      { ...sources[1], id: "one", models: [{ id: 'two",same', displayName: "Second model", reasoningEfforts: [] }, { id: "same", displayName: "Same ID elsewhere", reasoningEfforts: [] }] },
    ];
    const props = { ...defaults(), sources: catalog, value: null, onHiddenModelsChange: vi.fn().mockResolvedValue(undefined) };
    render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    await user.click(screen.getByRole("switch", { name: "Show First model from Personal" }));
    expect(props.onHiddenModelsChange).toHaveBeenCalledWith([JSON.stringify(['one",two', "same"])]);
    await user.click(screen.getByRole("button", { name: "Back to picker" }));
    expect(screen.queryByRole("button", { name: /^First model/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Second model/ })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: /^Same ID elsewhere/ }));
    await user.click(apply());
    expect(props.onChoose).toHaveBeenCalledWith({ providerProfileId: "one", model: "same", reasoningEffort: null });
  });

  it("selects the search-displayed visibility set and shows all without choosing or refreshing", async () => {
    const props = { ...defaults(), onHiddenModelsChange: vi.fn().mockResolvedValue(undefined) };
    render(<ComposerModelControls {...props} hiddenModelKeys={['["retired","model"]']} />);
    const user = await expand();
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    await user.type(screen.getByLabelText("Search models"), "luna");
    await user.click(screen.getByRole("button", { name: "Select displayed" }));
    expect(props.onHiddenModelsChange).toHaveBeenLastCalledWith([
      '["retired","model"]', '["account","sol"]', '["local","installed"]', '["offline","sol"]',
    ]);
    await user.clear(screen.getByLabelText("Search models"));
    expect(screen.getByRole("switch", { name: "Show Luna from Personal" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "Show Sol from Personal" })).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "Show all" }));
    expect(props.onHiddenModelsChange).toHaveBeenLastCalledWith([]);
    expect(screen.getAllByRole("switch").every(input => (input as HTMLInputElement).checked)).toBe(true);
    expect(props.onChoose).not.toHaveBeenCalled();
    expect(props.onRefresh).not.toHaveBeenCalled();
  });

  it("honors incoming visibility changes and retains a hidden choice on opening", async () => {
    const props = defaults();
    const { rerender } = render(<ComposerModelControls {...props} hiddenModelKeys={['["account","luna"]', '["account","sol"]']} />);
    const user = await expand();
    expect(screen.getByRole("button", { name: /Sol.*Selected.*Hidden/ })).toBeEnabled();
    expect(screen.queryByRole("button", { name: /^Luna/ })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Model")).toHaveValue("sol");
    rerender(<ComposerModelControls {...props} hiddenModelKeys={[]} />);
    expect(screen.getByRole("button", { name: /^Luna/ })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    expect(screen.getByRole("switch", { name: "Show Sol from Personal" })).toBeChecked();
    expect(props.onChoose).not.toHaveBeenCalled();
  });

  it("supports older callers with view-only visibility and never writes browser storage", async () => {
    const storage = vi.spyOn(Storage.prototype, "setItem");
    try {
      const props = defaults();
      render(<ComposerModelControls {...props} />);
      const user = await expand();
      await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
      await user.click(screen.getByRole("switch", { name: "Show Luna from Personal" }));
      expect(screen.getByRole("status")).toHaveTextContent("Model visibility changed for this view");
      await user.click(screen.getByRole("button", { name: "Back to picker" }));
      expect(screen.queryByRole("button", { name: /^Luna/ })).not.toBeInTheDocument();
      expect(storage).not.toHaveBeenCalled();
      expect(props.onChoose).not.toHaveBeenCalled();
      expect(props.onRefresh).not.toHaveBeenCalled();
    } finally { storage.mockRestore(); }
  });

  it("keeps Luna/none as the parent's selected default without applying or fabricating effort", async () => {
    const props = { ...defaults(), sources: [{ ...sources[0], models: [{ id: "luna", displayName: "Luna", reasoningEfforts: ["none", "low"] }] }],
      value: { providerProfileId: "account", model: "luna", reasoningEffort: "none" } };
    render(<ComposerModelControls {...props} />);
    expect(screen.getByText("Luna · None (no reasoning)")).toBeVisible();
    await expand();
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("effort:none");
    expect(screen.queryByRole("option", { name: "high" })).not.toBeInTheDocument();
    expect(props.onChoose).not.toHaveBeenCalled();
    expect(props.onRefresh).not.toHaveBeenCalled();
  });

  it("labels billing from model, source, then explicit provider fallback without assuming unknown costs are free", async () => {
    type Metadata = { billing?: "membership" | "free" | "local" | "api-credits" | "unknown"; priceHint?: string };
    const catalog: (ComposerModelSource & Metadata)[] = [
      { ...sources[0], id: "membership", provider: "chatgpt-membership", models: [{ id: "a", displayName: "Member model", reasoningEfforts: [] }] },
      { ...sources[0], id: "free", provider: "openrouter-free", models: [{ id: "b", displayName: "Free model", reasoningEfforts: [] }] },
      { ...sources[0], id: "crow", provider: "crowbot", models: [{ id: "c", displayName: "Crow model", reasoningEfforts: [] }] },
      { ...sources[0], id: "local", provider: "lm-studio", models: [{ id: "d", displayName: "Local model", reasoningEfforts: [] }] },
      { ...sources[0], id: "llama", provider: "llama-cpp", models: [{ id: "e", displayName: "Llama model", reasoningEfforts: [] }] },
      { ...sources[0], id: "unknown", provider: "OpenRouter", models: [{ id: "f", displayName: "Unknown model", reasoningEfforts: [] }] },
      { ...sources[0], id: "override", provider: "chatgpt-membership", billing: "api-credits", models: [
        { id: "g", displayName: "Credit model", reasoningEfforts: [], priceHint: "$2 / million tokens" } as ComposerModelSource["models"][number] & Metadata,
        { id: "h", displayName: "Override model", reasoningEfforts: [], billing: "unknown" } as ComposerModelSource["models"][number] & Metadata,
      ] },
    ];
    render(<ComposerModelControls {...defaults()} sources={catalog} value={null} />);
    await expand();
    expect(within(screen.getByRole("button", { name: /^Member model/ })).getByText("Membership")).toBeVisible();
    for (const name of ["Free model", "Crow model"]) expect(within(screen.getByRole("button", { name: new RegExp(`^${name}`) })).getByText("Free")).toBeVisible();
    for (const name of ["Local model", "Llama model"]) expect(within(screen.getByRole("button", { name: new RegExp(`^${name}`) })).getByText("Local")).toBeVisible();
    expect(within(screen.getByRole("button", { name: /^Unknown model/ })).getByText("Cost unknown")).toBeVisible();
    expect(within(screen.getByRole("button", { name: /^Credit model/ })).getByText("API credits")).toBeVisible();
    expect(screen.getByText("$2 / million tokens")).toBeVisible();
    expect(within(screen.getByRole("button", { name: /^Override model/ })).getByText("Cost unknown")).toBeVisible();
  });

  it("opens by keyboard, focuses search, and dismisses by Escape, close, outside click and focus departure", async () => {
    render(<><ComposerModelControls {...defaults()} /><button type="button">Outside</button></>);
    const user = userEvent.setup();
    const trigger = screen.getByRole("button", { name: /Model & effort/ });
    await user.tab();
    expect(trigger).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByLabelText("Search models")).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(trigger).toHaveFocus();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("region", { name: "Model picker" })).not.toBeInTheDocument();
    await user.click(trigger);
    await user.click(screen.getByRole("button", { name: "Close model picker" }));
    expect(trigger).toHaveFocus();
    await user.click(trigger);
    await user.click(screen.getByRole("button", { name: "Outside" }));
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    await user.click(trigger);
    act(() => screen.getByRole("button", { name: "Outside" }).focus());
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("button", { name: "Outside" })).toHaveFocus();
  });

  it("does not submit a surrounding composer when searching with Enter or changing visibility", async () => {
    const submit = vi.fn(event => event.preventDefault());
    const props = { ...defaults(), onHiddenModelsChange: vi.fn().mockResolvedValue(undefined) };
    render(<form onSubmit={submit}><ComposerModelControls {...props} /></form>);
    const user = await expand();
    await user.type(screen.getByLabelText("Search models"), "luna{Enter}");
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    await user.click(screen.getByRole("switch", { name: "Show Luna from Personal" }));
    expect(submit).not.toHaveBeenCalled();
    expect(props.onChoose).not.toHaveBeenCalled();
    expect(props.onRefresh).not.toHaveBeenCalled();
  });

  it("serializes visibility writes, preserves the prior list on failure, and allows retry", async () => {
    const task = deferred();
    const props = { ...defaults(), onHiddenModelsChange: vi.fn().mockReturnValueOnce(task.promise).mockResolvedValue(undefined) };
    render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    const toggle = screen.getByRole("switch", { name: "Show Luna from Personal" });
    act(() => { fireEvent.click(toggle); fireEvent.click(toggle); });
    expect(props.onHiddenModelsChange).toHaveBeenCalledTimes(1);
    expect(toggle).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Saving model visibility");
    await act(async () => task.reject(new Error("Preferences unavailable")));
    expect(screen.getByRole("alert")).toHaveTextContent("Could not save model visibility: Preferences unavailable");
    expect(toggle).toBeChecked();
    expect(toggle).toBeEnabled();
    await user.click(toggle);
    expect(toggle).not.toBeChecked();
    expect(props.onHiddenModelsChange).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(props.onChoose).not.toHaveBeenCalled();
    expect(props.onRefresh).not.toHaveBeenCalled();
  });

  it("keeps newer incoming visibility when an older save completes", async () => {
    const task = deferred();
    const props = { ...defaults(), onHiddenModelsChange: vi.fn().mockReturnValue(task.promise) };
    const { rerender } = render(<ComposerModelControls {...props} />);
    const user = await expand();
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    await user.click(screen.getByRole("switch", { name: "Show Luna from Personal" }));
    rerender(<ComposerModelControls {...props} hiddenModelKeys={['["local","installed"]']} />);
    await act(async () => task.resolve());
    expect(screen.getByRole("switch", { name: "Show Luna from Personal" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "Show Installed model from Local" })).not.toBeChecked();
  });

  it("disables visibility actions while the root is busy and handles an empty filtered list", async () => {
    const props = { ...defaults(), onHiddenModelsChange: vi.fn().mockResolvedValue(undefined) };
    render(<ComposerModelControls {...props} busy />);
    const user = await expand();
    await user.click(screen.getByRole("button", { name: "Edit displayed models" }));
    expect(screen.getByRole("switch", { name: "Show Luna from Personal" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Show all" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Select displayed" })).toBeDisabled();
    await user.type(screen.getByLabelText("Search models"), "no-match");
    expect(screen.getByText(/No matching models/)).toBeVisible();
    expect(props.onHiddenModelsChange).not.toHaveBeenCalled();
  });

  it("contains the fallback popup within a small viewport and keeps it outside composer overflow", async () => {
    const width = vi.spyOn(window, "innerWidth", "get").mockReturnValue(280);
    const height = vi.spyOn(window, "innerHeight", "get").mockReturnValue(240);
    try {
      render(<div data-testid="composer"><ComposerModelControls {...defaults()} /></div>);
      await expand();
      const popup = screen.getByRole("region", { name: "Model picker" });
      expect(popup.parentElement).toBe(document.body);
      expect(screen.getByTestId("composer")).not.toContainElement(popup);
      expect(popup).toHaveStyle({ width: "264px", maxHeight: "224px", left: "8px", top: "8px" });
    } finally { width.mockRestore(); height.mockRestore(); }
  });

  it("uses the native popover when available without requiring Invoker support", async () => {
    const native = nativePopoverFixture(false);
    try {
      const { unmount } = render(<ComposerModelControls {...defaults()} />);
      const user = userEvent.setup();
      const trigger = screen.getByRole("button", { name: /Model & effort/ });
      expect(trigger).not.toHaveAttribute("commandfor");
      await user.click(trigger);
      expect(native.show).toHaveBeenCalledTimes(1);
      expect(screen.getByRole("region", { name: "Model picker" })).toHaveAttribute("popover", "auto");
      expect(screen.getByLabelText("Search models")).toHaveFocus();
      await user.keyboard("{Escape}");
      expect(native.hide).toHaveBeenCalledTimes(1);
      expect(trigger).toHaveAttribute("aria-expanded", "false");
      expect(trigger).toHaveFocus();
      unmount();
    } finally { native.restore(); }
  });

  it("enhances the trigger only with supported Invoker commands and synchronizes native dismissal", async () => {
    const native = nativePopoverFixture(true);
    try {
      const { unmount } = render(<ComposerModelControls {...defaults()} />);
      const trigger = screen.getByRole("button", { name: /Model & effort/ });
      const popup = document.getElementById(trigger.getAttribute("aria-controls")!)!;
      expect(trigger).toHaveAttribute("commandfor", popup.id);
      expect(trigger).toHaveAttribute("command", "toggle-popover");
      // The browser executes this builtin command, then emits beforetoggle.
      act(() => native.toggle(popup, "open"));
      expect(trigger).toHaveAttribute("aria-expanded", "true");
      expect(native.show).not.toHaveBeenCalled();
      expect(screen.getByLabelText("Search models")).toHaveFocus();
      act(() => native.toggle(popup, "closed"));
      expect(trigger).toHaveAttribute("aria-expanded", "false");
      expect(trigger).toHaveFocus();
      unmount();
    } finally { native.restore(); }
  });

  it("applies explicit none separately from provider default", async () => {
    const props = defaults();
    const noneSources: ComposerModelSource[] = [{ ...sources[0], models: [
      { id: "luna", displayName: "Luna", reasoningEfforts: ["none", "low", "high"] },
    ] }];
    render(<ComposerModelControls {...props} sources={noneSources}
      value={{providerProfileId:"account",model:"luna",reasoningEffort:"low"}} />);
    const user = await expand();
    const effort = screen.getByLabelText("Reasoning effort");
    expect(within(effort).getByRole("option", { name: "None (no reasoning)" })).toHaveValue("effort:none");
    await user.selectOptions(effort, "effort:none");
    await user.click(apply());
    expect(props.onChoose).toHaveBeenLastCalledWith({ providerProfileId:"account",model:"luna",reasoningEffort:"none" });
    await user.selectOptions(effort, "default");
    await user.click(apply());
    expect(props.onChoose).toHaveBeenLastCalledWith({ providerProfileId:"account",model:"luna",reasoningEffort:null });
  });

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

  it("provides a keyboard-focusable popup and labels without submitting an enclosing composer", async () => {
    const submit = vi.fn(event => event.preventDefault());
    render(<form onSubmit={submit}><ComposerModelControls {...defaults()} /></form>);
    const user = userEvent.setup();
    await user.tab();
    expect(screen.getByText("Model & effort").closest("button")).toHaveFocus();
    await user.click(screen.getByText("Model & effort"));
    await user.click(screen.getByText("Selection details"));
    expect(screen.getByText(/Applies to the next message, not running tasks\./)).toBeVisible();
    expect(screen.getByText(/selected provider receives this conversation's message history/)).toBeVisible();
    screen.getByLabelText("Provider/account").focus();
    await user.tab();
    expect(screen.getByLabelText("Model")).toHaveFocus();
    await user.click(apply());
    expect(submit).not.toHaveBeenCalled();
  });
});
