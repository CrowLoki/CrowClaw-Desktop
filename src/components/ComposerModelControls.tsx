import { useEffect, useId, useRef, useState } from "react";
import type { ConversationModelChoice, ComposerModelSource } from "../gateway/composerContracts";
import "./ComposerModelControls.css";

type Props = {
  value: ConversationModelChoice | null;
  sources: ComposerModelSource[];
  busy?: boolean;
  error?: string | null;
  onChoose: (choice: ConversationModelChoice) => Promise<void>;
  onRefresh: (sourceId: string) => Promise<void>;
};

const emptyChoice: ConversationModelChoice = { providerProfileId: "", model: "", reasoningEffort: null };

export function ComposerModelControls({ value, sources, busy = false, error, onChoose, onRefresh }: Props) {
  const id = useId();
  const valueKey = JSON.stringify(value);
  const latestValueKey = useRef(valueKey);
  latestValueKey.current = valueKey;
  const [draft, setDraft] = useState<ConversationModelChoice>(value ?? emptyChoice);
  const [pending, setPending] = useState<"choose" | "refresh" | null>(null);
  const operation = useRef(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [notice, setNotice] = useState("");

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
  const summary = value ? `${appliedModel?.displayName ?? value.model} · ${value.reasoningEffort ?? "Provider default"}` : "Choose a model";
  const warning = invalidSource ? "This provider/account is unavailable or disconnected. Choose a connected source or refresh it."
    : invalidModel ? "This model is no longer available. Choose an available model."
    : invalidEffort ? "This effort is no longer supported. Choose a supported effort or Provider default." : null;

  function stage(next: ConversationModelChoice) {
    setDraft(next);
    setFailure(null);
    setNotice("Staged changes — use the button below to apply them.");
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

  return <details className="composer-model-controls">
    <summary tabIndex={0}><span>Model &amp; effort</span><span className="composer-model-controls__summary">{summary}</span></summary>
    <div className="composer-model-controls__panel" aria-busy={locked}>
      <p id={`${id}-scope`}>Applies to the next message, not running tasks. The selected provider receives this conversation's message history.</p>
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
          {source?.models.map(item => <option key={item.id} value={item.id}>{item.displayName}</option>)}
        </select>
        <label htmlFor={`${id}-effort`}>Reasoning effort</label>
        <select id={`${id}-effort`} value={invalidEffort ? "unavailable" : draft.reasoningEffort === null ? "default" : `effort:${draft.reasoningEffort}`} disabled={locked || !model || source?.status !== "ready"}
          aria-invalid={invalidEffort || undefined} aria-describedby={invalidEffort ? `${id}-warning` : undefined}
          onChange={event => stage({ ...draft, reasoningEffort: event.target.value === "default" ? null : event.target.value.slice(7) })}>
          <option value="default">Provider default</option>
          {invalidEffort && <option value="unavailable" disabled>Previous effort unavailable</option>}
          {efforts.map(effort => <option key={effort} value={`effort:${effort}`}>{effort}</option>)}
        </select>
        <div className="composer-model-controls__actions">
          <button type="button" disabled={locked || !source} onClick={() => void run("refresh")}>{pending === "refresh" ? "Refreshing…" : "Refresh models"}</button>
          <button type="button" className="composer-model-controls__apply" disabled={locked || !valid} onClick={() => void run("choose")}>{pending === "choose" ? "Applying…" : "Use for next message"}</button>
        </div>
      </fieldset>
      {warning && <p id={`${id}-warning`} role="alert">{warning}</p>}
      {error && <p role="alert">{error}</p>}
      {failure && <p role="alert">{failure}</p>}
      <p role="status">{pending === "choose" ? "Applying selection…" : pending === "refresh" ? "Refreshing models…" : busy ? "Model controls are busy." : notice}</p>
    </div>
  </details>;
}
