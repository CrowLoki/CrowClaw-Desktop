import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ConversationModelChoice, ComposerModelSource } from "../gateway/composerContracts";
import "./ComposerModelControls.css";

type Props = {
  value: ConversationModelChoice | null;
  sources: ComposerModelSource[];
  busy?: boolean;
  error?: string | null;
  onChoose: (choice: ConversationModelChoice) => Promise<void>;
  onRefresh: (sourceId: string) => Promise<void>;
  hiddenModelKeys?: string[];
  onHiddenModelsChange?: (keys: string[]) => Promise<void>;
};

const emptyChoice: ConversationModelChoice = { providerProfileId: "", model: "", reasoningEffort: null };
type Billing = "membership" | "free" | "local" | "api-credits" | "unknown";
// Structural metadata keeps this component compatible with older catalog contracts.
type BillingMetadata = { billing?: Billing; priceHint?: string };
const billingLabels: Record<Billing, string> = {
  membership: "Membership", free: "Free", local: "Local", "api-credits": "API credits", unknown: "Cost unknown",
};
const modelKey = (sourceId: string, modelId: string) => JSON.stringify([sourceId, modelId]);
function billingFor(source: ComposerModelSource, model: BillingMetadata): Billing {
  if (model.billing) return model.billing;
  const explicit = (source as ComposerModelSource & BillingMetadata).billing;
  if (explicit) return explicit;
  const provider = source.provider.toLowerCase().replace(/[ _]/g, "-");
  if (provider === "chatgpt" || provider === "chatgpt-membership") return "membership";
  if (provider === "openrouter-free" || provider === "crowbot") return "free";
  if (["lm-studio", "ollama", "llama-cpp"].includes(provider)) return "local";
  return "unknown";
}

export function ComposerModelControls({ value, sources, busy = false, error, onChoose, onRefresh, hiddenModelKeys = [], onHiddenModelsChange }: Props) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const [query, setQuery] = useState("");
  const hiddenKey = JSON.stringify(hiddenModelKeys);
  const latestHiddenKey = useRef(hiddenKey);
  latestHiddenKey.current = hiddenKey;
  const [hidden, setHidden] = useState(hiddenModelKeys);
  const nativePopover = typeof HTMLElement !== "undefined" && "popover" in HTMLElement.prototype && typeof HTMLElement.prototype.showPopover === "function";
  const nativeInvoker = nativePopover && "commandForElement" in HTMLButtonElement.prototype;
  const valueKey = JSON.stringify(value);
  const latestValueKey = useRef(valueKey);
  latestValueKey.current = valueKey;
  const [draft, setDraft] = useState<ConversationModelChoice>(value ?? emptyChoice);
  const [pending, setPending] = useState<"choose" | "refresh" | "visibility" | null>(null);
  const operation = useRef(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [notice, setNotice] = useState("");

  useEffect(() => { setHidden(JSON.parse(hiddenKey) as string[]); }, [hiddenKey]);

  function dismiss(returnFocus = true) {
    if (nativePopover && panel.current?.matches(":popover-open")) panel.current.hidePopover();
    setOpen(false);
    if (returnFocus) trigger.current?.focus();
  }

  useEffect(() => {
    const element = panel.current;
    if (!element || !nativePopover) return;
    const sync = (event: Event) => {
      const opening = (event as Event & { newState: string }).newState === "open";
      setOpen(opening);
      if (!opening && element.contains(document.activeElement)) trigger.current?.focus();
    };
    // React can flush effects while beforetoggle's native show operation is
    // still running. Synchronize only after the browser finishes that operation.
    element.addEventListener("toggle", sync);
    return () => element.removeEventListener("toggle", sync);
  }, [nativePopover]);

  useEffect(() => {
    if (!open) return;
    const element = panel.current!;
    if (nativePopover && !element.matches(":popover-open")) element.showPopover();
    function position() {
      const rect = trigger.current!.getBoundingClientRect();
      const viewport = window.visualViewport;
      const leftEdge = (viewport?.offsetLeft ?? 0) + 8;
      const topEdge = (viewport?.offsetTop ?? 0) + 8;
      const width = Math.max(0, (viewport?.width ?? window.innerWidth) - 16);
      const height = Math.max(0, (viewport?.height ?? window.innerHeight) - 16);
      element.style.width = `${Math.min(420, width)}px`;
      element.style.maxHeight = `${Math.min(560, height)}px`;
      const panelHeight = Math.min(element.getBoundingClientRect().height || 520, height);
      element.style.left = `${Math.max(leftEdge, Math.min(rect.left, leftEdge + width - Math.min(420, width)))}px`;
      const above = rect.top - panelHeight - 6;
      element.style.top = `${Math.max(topEdge, Math.min(above >= topEdge ? above : rect.bottom + 6, topEdge + height - panelHeight))}px`;
    }
    position();
    search.current?.focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); dismiss(); }
    };
    const outside = (event: Event) => {
      const target = event.target as Node;
      if (!element.contains(target) && !trigger.current?.contains(target)) dismiss(event.type !== "focusin");
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(position);
    observer?.observe(element);
    document.addEventListener("keydown", keydown);
    document.addEventListener("pointerdown", outside);
    document.addEventListener("focusin", outside);
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    window.visualViewport?.addEventListener("resize", position);
    window.visualViewport?.addEventListener("scroll", position);
    return () => {
      observer?.disconnect();
      document.removeEventListener("keydown", keydown);
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("focusin", outside);
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
      window.visualViewport?.removeEventListener("resize", position);
      window.visualViewport?.removeEventListener("scroll", position);
    };
  }, [open, nativePopover]);

  useEffect(() => {
    if (!trigger.current || !nativeInvoker) return;
    trigger.current.setAttribute("commandfor", `${id}-panel`);
    trigger.current.setAttribute("command", "toggle-popover");
  }, [id, nativeInvoker]);

  useEffect(() => {
    setDraft((JSON.parse(valueKey) as ConversationModelChoice | null) ?? emptyChoice);
    setFailure(null);
    setNotice("");
  }, [valueKey]);

  const source = sources.find(item => item.id === draft.providerProfileId);
  const model = source?.models.find(item => item.id === draft.model);
  const efforts = [...new Set(model?.reasoningEfforts.filter(Boolean) ?? [])];
  const invalidSource = Boolean(draft.providerProfileId) && (!source || source.status !== "ready");
  const invalidModel = Boolean(draft.model) && !model;
  const invalidEffort = draft.reasoningEffort !== null && !efforts.includes(draft.reasoningEffort);
  const valid = Boolean(source?.status === "ready" && model && !invalidEffort);
  const locked = busy || pending !== null;
  const appliedSource = sources.find(item => item.id === value?.providerProfileId);
  const appliedModel = appliedSource?.models.find(item => item.id === value?.model);
  const summary = value ? `${appliedModel?.displayName ?? value.model} · ${value.reasoningEffort === "none" ? "None (no reasoning)" : value.reasoningEffort ?? "Provider default"}` : "Choose a model";
  const matches = (item: ComposerModelSource, entry: ComposerModelSource["models"][number]) =>
    `${item.label} ${item.provider} ${entry.id} ${entry.displayName}`.toLowerCase().includes(query.trim().toLowerCase());
  const groups = sources.map(item => ({ source: item, models: item.models.filter(entry =>
    matches(item, entry) && (editing || !hidden.includes(modelKey(item.id, entry.id)) ||
      (item.id === draft.providerProfileId && entry.id === draft.model) || (item.id === value?.providerProfileId && entry.id === value.model))) }));
  const warning = invalidSource ? "This provider/account is unavailable or disconnected. Choose a connected source or refresh it."
    : invalidModel ? "This model is no longer available. Choose an available model."
    : invalidEffort ? "This effort is no longer supported. Choose a supported effort or Provider default." : null;

  function stage(next: ConversationModelChoice) {
    setDraft(next);
    setFailure(null);
    setNotice("Staged changes — use the button below to apply them.");
  }

  async function changeVisibility(keys: string[]) {
    if (busy || operation.current) return;
    operation.current = true;
    setPending("visibility");
    setFailure(null);
    setNotice("");
    const startedHiddenKey = hiddenKey;
    try {
      await onHiddenModelsChange?.(keys);
      if (latestHiddenKey.current === startedHiddenKey) setHidden(keys);
      setNotice(onHiddenModelsChange ? "Model visibility saved." : "Model visibility changed for this view.");
    } catch (cause) {
      const detail = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "Please try again.";
      setFailure(`Could not save model visibility: ${detail}`);
    } finally {
      operation.current = false;
      setPending(null);
    }
  }

  async function run(kind: "choose" | "refresh") {
    if (busy || operation.current || (kind === "choose" ? !valid : !source)) return;
    operation.current = true;
    setPending(kind);
    setFailure(null);
    setNotice("");
    const startedValueKey = valueKey;
    try {
      if (kind === "choose") await onChoose({ ...draft });
      else await onRefresh(source!.id);
      if (latestValueKey.current === startedValueKey) {
        setNotice(kind === "choose" ? "Selection applied for the next message." : "Models refreshed. Review your choices before applying.");
      }
    } catch (cause) {
      if (latestValueKey.current === startedValueKey) {
        const detail = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "Please try again.";
        setFailure(`${kind === "choose" ? "Could not apply selection" : "Could not refresh models"}: ${detail}`);
      }
    } finally {
      operation.current = false;
      setPending(null);
    }
  }

  return <div className="composer-model-controls">
    <button ref={trigger} type="button" className="composer-model-controls__trigger" aria-expanded={open} aria-controls={`${id}-panel`}
      onClick={() => { if (!nativeInvoker) { if (open) dismiss(); else setOpen(true); } }}>
      <span>Model &amp; effort</span><span className="composer-model-controls__summary">{summary}</span><span aria-hidden="true">⌃</span>
    </button>
    {createPortal(<div ref={panel} id={`${id}-panel`} popover={nativePopover ? "auto" : undefined} hidden={!nativePopover && !open}
      data-open={open} className="composer-model-controls composer-model-controls__panel" aria-busy={locked} role="region" aria-label="Model picker"
      onKeyDown={event => { if (event.key === "Enter" && event.target instanceof HTMLInputElement && !event.nativeEvent.isComposing) event.preventDefault(); }}>
      <div className="composer-model-controls__header"><strong>{editing ? "Models to display" : "Choose a model"}</strong>
        <button type="button" onClick={() => dismiss()} aria-label="Close model picker">×</button></div>
      <label htmlFor={`${id}-search`}>Search models</label>
      <input ref={search} id={`${id}-search`} type="search" name="model-search" autoComplete="off" autoCapitalize="none" spellCheck={false}
        value={query} onChange={event => setQuery(event.target.value)} />
      <div className="composer-model-controls__toolbar">
        <button type="button" aria-pressed={editing} onClick={() => setEditing(!editing)}>{editing ? "Back to picker" : "Edit displayed models"}</button>
        {editing && <>
          <button type="button" disabled={locked} onClick={() => void changeVisibility([])}>Show all</button>
          <button type="button" disabled={locked} onClick={() => {
            const displayed = new Set(groups.flatMap(group => group.models.map(entry => modelKey(group.source.id, entry.id))));
            const catalog = sources.flatMap(item => item.models.map(entry => modelKey(item.id, entry.id)));
            void changeVisibility([...new Set([...hidden.filter(key => !catalog.includes(key)), ...catalog.filter(key => !displayed.has(key))])]);
          }}>Select displayed</button>
        </>}
      </div>
      {editing && <p>Visibility only changes this list. Your selected model and default stay the same.</p>}
      <div className="composer-model-controls__catalog">
        {groups.filter(group => group.models.length).map(({ source: item, models }) => <fieldset key={item.id} className="composer-model-controls__group">
          <legend>{item.label} · {item.provider}{item.status !== "ready" ? " (disconnected)" : ""}</legend>
          {models.map(entry => {
            const key = modelKey(item.id, entry.id);
            const selected = item.id === value?.providerProfileId && entry.id === value.model;
            const staged = item.id === draft.providerProfileId && entry.id === draft.model;
            const metadata = entry as typeof entry & BillingMetadata;
            const content = <><span className="composer-model-controls__model-name">{entry.displayName}
              <small>{entry.id}{selected ? " · Selected" : staged ? " · Staged" : ""}{hidden.includes(key) ? " · Hidden from list" : ""}</small></span>
              <span className="composer-model-controls__billing" data-billing={billingFor(item, metadata)}>{billingLabels[billingFor(item, metadata)]}{metadata.priceHint && <small>{metadata.priceHint}</small>}</span></>;
            return editing ? <label key={key} className="composer-model-controls__model">
              <input type="checkbox" role="switch" name="model-visible" aria-label={`Show ${entry.displayName} from ${item.label}`} checked={!hidden.includes(key)} disabled={locked}
                onChange={event => void changeVisibility(event.target.checked ? hidden.filter(existing => existing !== key) : [...hidden, key])} />{content}
            </label> : <button key={key} type="button" className="composer-model-controls__model" aria-pressed={staged} disabled={locked || item.status !== "ready"}
              onClick={() => stage({ providerProfileId: item.id, model: entry.id, reasoningEffort: staged ? draft.reasoningEffort : null })}>{content}</button>;
          })}
        </fieldset>)}
        {!groups.some(group => group.models.length) && <p>No matching models. Try another search or edit displayed models.</p>}
      </div>
      <p id={`${id}-scope`}>Applies to the next message, not running tasks. The selected provider receives this conversation's message history.</p>
      {!editing && <>
      <details className="composer-model-controls__details"><summary>Selection details</summary>
      <fieldset disabled={locked} aria-describedby={`${id}-scope`}>
        <legend>Next message model</legend>
        <label htmlFor={`${id}-source`}>Provider/account</label>
        <select id={`${id}-source`} value={draft.providerProfileId} aria-invalid={invalidSource || undefined} aria-describedby={invalidSource ? `${id}-warning` : undefined}
          onChange={event => stage({ providerProfileId: event.target.value, model: "", reasoningEffort: null })}>
          <option value="">Choose a provider/account</option>
          {draft.providerProfileId && !source && <option value={draft.providerProfileId} disabled>Unavailable provider/account</option>}
          {sources.map(item => <option key={item.id} value={item.id} disabled={item.status !== "ready"}>{item.label} · {item.provider}{item.status !== "ready" ? " (disconnected)" : ""}</option>)}
        </select>
        <label htmlFor={`${id}-model`}>Model</label>
        <select id={`${id}-model`} value={draft.model} disabled={locked || source?.status !== "ready"} aria-invalid={invalidModel || undefined} aria-describedby={invalidModel ? `${id}-warning` : undefined}
          onChange={event => stage({ ...draft, model: event.target.value, reasoningEffort: null })}>
          <option value="">Choose a model</option>
          {invalidModel && <option value={draft.model} disabled>{draft.model} (unavailable)</option>}
          {source?.models.filter(item => !hidden.includes(modelKey(source.id, item.id)) || item.id === draft.model || (source.id === value?.providerProfileId && item.id === value.model)).map(item => <option key={item.id} value={item.id}>{item.displayName}</option>)}
        </select>
      </fieldset>
      </details>
      <fieldset disabled={locked} aria-describedby={`${id}-scope`}>
        <label htmlFor={`${id}-effort`}>Reasoning effort</label>
        <select id={`${id}-effort`} value={invalidEffort ? "unavailable" : draft.reasoningEffort === null ? "default" : `effort:${draft.reasoningEffort}`} disabled={locked || !model || source?.status !== "ready"}
          aria-invalid={invalidEffort || undefined} aria-describedby={invalidEffort ? `${id}-warning` : undefined}
          onChange={event => stage({ ...draft, reasoningEffort: event.target.value === "default" ? null : event.target.value.slice(7) })}>
          <option value="default">Provider default</option>
          {invalidEffort && <option value="unavailable" disabled>Previous effort unavailable</option>}
          {efforts.map(effort => <option key={effort} value={`effort:${effort}`}>{effort === "none" ? "None (no reasoning)" : effort}</option>)}
        </select>
        <div className="composer-model-controls__actions">
          <button type="button" disabled={locked || !source} onClick={() => void run("refresh")}>{pending === "refresh" ? "Refreshing…" : "Refresh models"}</button>
          <button type="button" className="composer-model-controls__apply" disabled={locked || !valid} onClick={() => void run("choose")}>{pending === "choose" ? "Applying…" : "Use for next message"}</button>
        </div>
      </fieldset>
      </>}
      {warning && <p id={`${id}-warning`} role="alert">{warning}</p>}
      {error && <p role="alert">{error}</p>}
      {failure && <p role="alert">{failure}</p>}
      <p role="status">{pending === "choose" ? "Applying selection…" : pending === "refresh" ? "Refreshing models…" : pending === "visibility" ? "Saving model visibility…" : busy ? "Model controls are busy." : notice}</p>
    </div>, document.body)}
  </div>;
}
