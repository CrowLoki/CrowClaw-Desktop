import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import type {
  CrowClawGateway, EvolutionObservation, EvolutionProposal, EvolutionRating,
  EvolutionSnapshot, ModelConnection,
} from "../gateway/contracts";

type Props = { gateway: CrowClawGateway; connection: ModelConnection | null; developmentPreview?: boolean };
type Preference = "baseline" | "candidate" | "tie" | "neither";
const ratingLabels: Record<EvolutionRating, string> = { useful: "Useful", needs_improvement: "Needs improvement", uncertain: "Uncertain" };
const preferences: Array<{ value: Preference; label: string }> = [
  { value: "baseline", label: "Prefer baseline" }, { value: "candidate", label: "Prefer candidate" },
  { value: "tie", label: "Tie" }, { value: "neither", label: "Neither" },
];

function errorMessage(cause: unknown) { return cause instanceof Error ? cause.message : String(cause); }
function dateLabel(value: number) { return new Date(value).toLocaleString(); }

function ReflectionInputs({ context }: { context:Record<string,unknown> }) {
  const evidence=typeof context.selectedEvidence==="object" && context.selectedEvidence!==null ? context.selectedEvidence as Record<string,unknown> : null;
  const feedback=typeof evidence?.feedback==="object" && evidence.feedback!==null ? evidence.feedback as Record<string,unknown> : null;
  return <details><summary>Original reflection goal and feedback</summary>
    <p className="evolution-text">Goal: {typeof context.goal==="string" ? context.goal : "not recorded"}</p>
    <p className="evolution-text">Feedback: {typeof feedback?.rating==="string" ? feedback.rating.replace(/_/g," ") : "No quality rating supplied"}{typeof feedback?.note==="string" && feedback.note ? ` — ${feedback.note}` : ""}</p>
  </details>;
}

function TaskFeedback({ observation, busy, onSave }: {
  observation: EvolutionObservation; busy: boolean;
  onSave: (taskId: string, rating: EvolutionRating, note: string) => void;
}) {
  const id = useId();
  const [rating, setRating] = useState<EvolutionRating | "">(observation.feedback?.rating ?? "");
  const [note, setNote] = useState(observation.feedback?.note ?? "");
  function submit(event: FormEvent) {
    event.preventDefault();
    if (rating && !busy) onSave(observation.taskId, rating, note);
  }
  return <article className="evolution-card" aria-label={`Task: ${observation.title}`}>
    <h3>{observation.title}</h3>
    <p>Outcome: {observation.outcome} · Guidelines: {observation.guidelineRevision === null ? "not recorded" : `revision ${observation.guidelineRevision}`}</p>
    <time dateTime={new Date(observation.updatedAtMs).toISOString()}>{dateLabel(observation.updatedAtMs)}</time>
    {observation.feedback && <p>Saved feedback: {ratingLabels[observation.feedback.rating]}{observation.feedback.note ? ` — ${observation.feedback.note}` : ""}</p>}
    <form onSubmit={submit} className="evolution-form">
      <fieldset className="evolution-options">
        <legend>Quality feedback</legend>
        {(Object.entries(ratingLabels) as Array<[EvolutionRating, string]>).map(([value, label]) =>
          <label key={value}><input type="radio" name={`${id}-rating`} value={value} checked={rating === value} onChange={() => setRating(value)} required />{label}</label>)}
      </fieldset>
      <label className="field" htmlFor={`${id}-note`}>Feedback note (optional)</label>
      <textarea id={`${id}-note`} name="note" value={note} rows={2} onChange={(event) => setNote(event.target.value)} />
      <button type="submit" className="button button--secondary" disabled={busy || !rating}>Save feedback</button>
    </form>
  </article>;
}

export function EvolutionView({ gateway, connection, developmentPreview = false }: Props) {
  const [snapshot, setSnapshot] = useState<EvolutionSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pending, setPending] = useState<{ label: string; requestId?: string } | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [title, setTitle] = useState("");
  const [rationale, setRationale] = useState("");
  const [manualInstructions, setManualInstructions] = useState("");
  const [manualTaskId, setManualTaskId] = useState("");
  const [manualBase, setManualBase] = useState<number | null>(null);
  const [reflectionTask, setReflectionTask] = useState("");
  const [goal, setGoal] = useState("");
  const [editor, setEditor] = useState({ id: "", instructions: "" });
  const [prompt, setPrompt] = useState("");
  const [restoreNumber, setRestoreNumber] = useState("");
  const alive = useRef(false);
  const operation = useRef(false);
  const activeRequest = useRef<string | undefined>(undefined);
  const editorVersion = useRef(0);
  const loadVersion = useRef(0);
  const connected = connection?.status === "connected";
  const busy = loading || pending !== null;
  const proposal = snapshot?.proposals.find((item) => item.id === editor.id);
  const isDraft = proposal?.status === "draft";

  const refresh = useCallback(async () => {
    const version = ++loadVersion.current;
    const value = await gateway.evolutionSnapshot();
    if (alive.current && version === loadVersion.current) setSnapshot(value);
  }, [gateway]);

  useEffect(() => {
    alive.current = true;
    setLoading(true);
    setError(null);
    void refresh().catch((cause) => { if (alive.current) setError(errorMessage(cause)); })
      .finally(() => { if (alive.current) setLoading(false); });
    return () => { alive.current = false; ++loadVersion.current; };
  }, [refresh]);

  async function perform(label: string, work: () => Promise<string>, requestId?: string) {
    if (operation.current || loading) return;
    operation.current = true;
    activeRequest.current = requestId;
    setPending({ label, requestId }); setError(null); setNotice(null);
    try {
      const message = await work();
      if (alive.current) setNotice(message);
      await refresh();
    } catch (cause) {
      if (alive.current) setError(errorMessage(cause));
    } finally {
      operation.current = false;
      activeRequest.current = undefined;
      if (alive.current) { setPending(null); setCancelling(false); }
    }
  }

  function selectProposal(value: EvolutionProposal) {
    ++editorVersion.current;
    setEditor({ id: value.id, instructions: value.instructions });
  }

  function manualEdited() {
    setManualBase((current) => current ?? snapshot?.active.revision ?? null);
  }

  function saveDraft(event: FormEvent) {
    event.preventDefault();
    if (!snapshot || !title.trim() || !rationale.trim() || !manualInstructions.trim()) return;
    const request = { title, rationale, instructions: manualInstructions, sourceTaskIds: manualTaskId ? [manualTaskId] : [], baseRevision: manualBase ?? snapshot.active.revision };
    const version = editorVersion.current;
    void perform("Saving proposal…", async () => {
      const value = await gateway.draftEvolution(request);
      if (alive.current && version === editorVersion.current) selectProposal(value);
      return "Proposal saved for review. It is not active.";
    });
  }

  function reflect(event: FormEvent) {
    event.preventDefault();
    if (!connected || !reflectionTask || !goal.trim()) return;
    const request = { requestId: crypto.randomUUID(), taskId: reflectionTask, goal };
    const version = editorVersion.current;
    void perform("Requesting reflection…", async () => {
      const value = await gateway.reflectEvolution(request);
      if (alive.current && version === editorVersion.current) selectProposal(value);
      return "Reflection proposal saved for review. It is not active.";
    }, request.requestId);
  }

  function evaluate(event: FormEvent) {
    event.preventDefault();
    if (!connected || !isDraft || !editor.instructions.trim() || !prompt.trim()) return;
    const request = { requestId: crypto.randomUUID(), proposalId: editor.id, instructions: editor.instructions, prompt };
    void perform("Comparing responses…", async () => {
      await gateway.evaluateEvolution(request);
      return "Comparison recorded. Choose the response you prefer.";
    }, request.requestId);
  }

  function decide(decision: "apply" | "reject") {
    if (!snapshot || !isDraft) return;
    const request = { id: editor.id, decision, instructions: editor.instructions, expectedRevision: snapshot.active.revision };
    const version = editorVersion.current;
    void perform(decision === "apply" ? "Applying proposal…" : "Rejecting proposal…", async () => {
      const value = await gateway.decideEvolution(request);
      const newerEdits = editorVersion.current !== version;
      return decision === "apply"
        ? `Guideline revision ${value.appliedRevision} applied.${newerEdits ? " Newer editor text was kept; it was not applied." : ""}`
        : "Proposal rejected. Active guidelines did not change.";
    });
  }

  function restoreRevision(revision: number) {
    if (!snapshot) return;
    const request = { revision, expectedRevision: snapshot.active.revision };
    void perform("Restoring guidelines…", async () => {
      const restored = await gateway.restoreEvolution(request);
      return `Revision ${revision} restored as new revision ${restored.revision}.`;
    });
  }

  async function cancel() {
    if (!pending?.requestId || cancelling) return;
    const requestId = pending.requestId;
    setCancelling(true);
    try {
      await gateway.cancelEvolution(requestId);
      if (alive.current && activeRequest.current === requestId) setNotice("Cancellation requested. Waiting for the model request to finish.");
    } catch (cause) {
      if (alive.current) { setError(errorMessage(cause)); setCancelling(false); }
    }
  }

  return <main className="section-view evolution-view" aria-labelledby="evolution-title">
    <header className="section-heading">
      <div><span className="eyebrow">Review and guide</span><h1 id="evolution-title">Evolution</h1>
        <p>Review task outcomes, compare guidelines and choose what CrowClaw adopts.</p></div>
      <button type="button" className="button button--secondary" disabled={busy} onClick={() => void perform("Refreshing…", async () => "Evolution history refreshed.")}>Refresh history</button>
    </header>
    <div className="evolution-content">
      <p className="evolution-explanation">Adopting a guideline sends it to the connected model in future tasks. A task success is not a quality rating. Tool approvals still apply.</p>
      {developmentPreview && <p className="evolution-explanation">Development simulation: proposals, responses and history stay in this browser session. No model is called; no quality improvement is proven.</p>}
      {loading && <p role="status">Loading evolution history…</p>}
      {pending && <div className="evolution-actions"><p role="status">{pending.label}</p>{pending.requestId && <button type="button" className="button button--danger-quiet" disabled={cancelling} onClick={() => void cancel()}>{cancelling ? "Cancelling…" : "Cancel model request"}</button>}</div>}
      {error && <p role="alert" className="inline-error">{error}</p>}
      {notice && <p role="status" className="evolution-notice">{notice}</p>}
      {snapshot && <>
        <section className="evolution-panel" aria-labelledby="active-guidelines-title">
          <h2 id="active-guidelines-title">Active guidelines · revision {snapshot.active.revision}</h2>
          <h3>{snapshot.active.title}</h3><p className="evolution-text">{snapshot.active.instructions || "No additional guidelines adopted."}</p>
        </section>
        <section className="evolution-panel" aria-labelledby="outcomes-title">
          <h2 id="outcomes-title">Recent terminal tasks (up to 50)</h2>
          {snapshot.observations.length === 0 && <p>No terminal task outcomes recorded yet.</p>}
          <div className="evolution-list">{snapshot.observations.map((observation) => <TaskFeedback key={observation.taskId} observation={observation} busy={busy} onSave={(taskId, rating, note) => void perform("Saving feedback…", async () => { await gateway.saveEvolutionFeedback({ taskId, rating, note }); return "Quality feedback saved."; })} />)}</div>
        </section>
        <section className="evolution-panel" aria-labelledby="manual-proposal-title">
          <h2 id="manual-proposal-title">Write a proposal</h2><p>Save a draft locally, including when the model is offline.</p>
          <form className="evolution-form" onSubmit={saveDraft}>
            <label className="field" htmlFor="evolution-draft-title">Proposal title (required)</label>
            <input id="evolution-draft-title" name="title" required value={title} onChange={(event) => { manualEdited(); setTitle(event.target.value); }} />
            <label className="field" htmlFor="evolution-draft-reason">Reason for change (required)</label>
            <textarea id="evolution-draft-reason" name="rationale" required rows={2} value={rationale} onChange={(event) => { manualEdited(); setRationale(event.target.value); }} />
            <label className="field" htmlFor="evolution-draft-instructions">Proposed instructions (required)</label>
            <textarea id="evolution-draft-instructions" name="instructions" required rows={3} value={manualInstructions} onChange={(event) => { manualEdited(); setManualInstructions(event.target.value); }} />
            <label className="field" htmlFor="evolution-draft-task">Related task (optional)</label>
            <select id="evolution-draft-task" name="sourceTaskId" value={manualTaskId} onChange={(event) => { manualEdited(); setManualTaskId(event.target.value); }}><option value="">No related task</option>{snapshot.observations.map((item) => <option key={item.taskId} value={item.taskId}>{item.title} · {item.outcome}</option>)}</select>
            <p>Based on revision {manualBase ?? snapshot.active.revision}.</p>
            {manualBase !== null && manualBase !== snapshot.active.revision && <div className="evolution-actions"><p>The active revision changed. Review it before updating this draft’s base.</p><button type="button" className="button button--secondary" disabled={busy} onClick={() => setManualBase(snapshot.active.revision)}>Use active revision {snapshot.active.revision} as draft base</button></div>}
            <button type="submit" className="button button--secondary" disabled={busy || !title.trim() || !rationale.trim() || !manualInstructions.trim()}>Save proposal</button>
          </form>
        </section>
        <section className="evolution-panel" aria-labelledby="reflection-title">
          <h2 id="reflection-title">Ask the model to reflect</h2><p>Sends the selected task context and your goal to the connected model only when you click.</p>
          {!connected && <p>Model offline. Connect a model to reflect or compare responses.</p>}
          <form className="evolution-form" onSubmit={reflect}>
            <label className="field" htmlFor="evolution-reflection-task">Task to reflect on (required)</label>
            <select id="evolution-reflection-task" name="taskId" required value={reflectionTask} onChange={(event) => setReflectionTask(event.target.value)}><option value="">Choose a terminal task</option>{snapshot.observations.map((item) => <option key={item.taskId} value={item.taskId}>{item.title} · {item.outcome}</option>)}</select>
            <label className="field" htmlFor="evolution-goal">Reflection goal (required)</label>
            <textarea id="evolution-goal" name="goal" required rows={2} value={goal} onChange={(event) => setGoal(event.target.value)} />
            <button type="submit" className="button button--secondary" disabled={busy || !connected || !reflectionTask || !goal.trim()}>Request model reflection</button>
          </form>
        </section>
        <section className="evolution-panel" aria-labelledby="candidate-title">
          <h2 id="candidate-title">Review a candidate</h2><p>Recent proposals · latest 100.</p>
          <label className="field" htmlFor="evolution-proposal">Proposal</label>
          <select id="evolution-proposal" name="proposalId" value={editor.id} onChange={(event) => { const value = snapshot.proposals.find((item) => item.id === event.target.value); if (value) selectProposal(value); else { ++editorVersion.current; setEditor({ id: "", instructions: "" }); } }}>
            <option value="">Choose a proposal</option>{snapshot.proposals.map((item) => <option key={item.id} value={item.id}>{item.title} · {item.status} · base {item.baseRevision}</option>)}
          </select>
          {proposal && <>
            <p>{proposal.rationale}</p><p>Status: {proposal.status} · Based on revision {proposal.baseRevision}</p>
            <p>{proposal.model === null ? "Manual proposal · no model used" : `Requested reflection model: ${proposal.model}`}</p>
            {proposal.model !== null && <p>Reported reflection model: {proposal.reportedModel || "not reported"}</p>}
            {proposal.reflectionContext && <ReflectionInputs context={proposal.reflectionContext} />}
            <p>Source tasks: {proposal.sourceTaskIds.map((id) => snapshot.observations.find((item) => item.taskId === id)?.title ?? id).join(", ") || "Manual proposal"}</p>
            {proposal.baseRevision !== snapshot.active.revision && isDraft && <p className="evolution-warning">This draft is based on an earlier revision. Refresh and review the active guidelines; applying a stale draft will be rejected.</p>}
            <form className="evolution-form" onSubmit={evaluate}>
              <label className="field" htmlFor="evolution-candidate-instructions">Candidate instructions</label>
              <textarea id="evolution-candidate-instructions" name="instructions" rows={4} value={editor.instructions} onChange={(event) => { ++editorVersion.current; setEditor((current) => ({ ...current, instructions: event.target.value })); }} />
              <p>Edits are sent when you compare or Apply. Comparing and voting do not activate them.</p>
              <label className="field" htmlFor="evolution-evaluation-prompt">Comparison prompt</label>
              <p>Runs this prompt twice with one frozen model connection, without tools.</p>
              <textarea id="evolution-evaluation-prompt" name="prompt" rows={2} value={prompt} onChange={(event) => setPrompt(event.target.value)} />
              <button type="submit" className="button button--secondary" disabled={busy || !connected || !isDraft || !editor.instructions.trim() || !prompt.trim()}>Compare baseline and candidate</button>
            </form>
            <div className="evolution-actions">
              <button type="button" className="button button--primary" disabled={busy || !isDraft || !editor.instructions.trim()} onClick={() => decide("apply")}>Apply proposal</button>
              <button type="button" className="button button--danger-quiet" disabled={busy || !isDraft} onClick={() => decide("reject")}>Reject proposal</button>
              <span>Expected active revision: {snapshot.active.revision}</span>
            </div>
          </>}
        </section>
        <section className="evolution-panel" aria-labelledby="comparisons-title">
          <h2 id="comparisons-title">Evaluated responses · latest 100</h2>
          {snapshot.evaluations.length === 0 && <p>No comparisons recorded yet.</p>}
          <div className="evolution-list">{snapshot.evaluations.map((evaluation) => <article key={evaluation.id} className="evolution-card" aria-label={`Comparison: ${evaluation.prompt}`}>
            <h3>{snapshot.proposals.find((item) => item.id === evaluation.proposalId)?.title ?? "Earlier proposal"} · baseline revision {evaluation.baselineRevision}</h3>
            <p>Requested comparison model: {evaluation.model}</p>
            <p>Reported baseline model: {evaluation.baselineModel || "not reported"} · Reported candidate model: {evaluation.candidateModel || "not reported"}</p>
            {evaluation.baselineModel && evaluation.candidateModel && evaluation.baselineModel !== evaluation.candidateModel && <p className="evolution-warning">The endpoint reported different models for these responses. This comparison does not isolate the guideline change.</p>}
            <time dateTime={new Date(evaluation.createdAtMs).toISOString()}>{dateLabel(evaluation.createdAtMs)}</time>
            <p className="evolution-text">Prompt: {evaluation.prompt}</p>
            <details><summary>Instructions used for this candidate</summary><p className="evolution-text">{evaluation.candidateInstructions}</p></details>
            <div className="evolution-comparison"><div><h4>Baseline response</h4><p className="evolution-text">{evaluation.baselineResponse}</p></div><div><h4>Candidate response</h4><p className="evolution-text">{evaluation.candidateResponse}</p></div></div>
            {evaluation.proposalId === editor.id && evaluation.candidateInstructions !== editor.instructions && <p className="evolution-warning">The current editor differs from this evaluated candidate.</p>}
            <fieldset className="evolution-options"><legend>Your response preference</legend>{preferences.map(({ value, label }) => <button key={value} type="button" className="button button--secondary" aria-pressed={evaluation.preference === value} disabled={busy} onClick={() => void perform("Saving preference…", async () => { await gateway.rateEvolutionEvaluation({ id: evaluation.id, preference: value }); return "Response preference saved. Active guidelines did not change."; })}>{label}</button>)}</fieldset>
          </article>)}</div>
        </section>
        <section className="evolution-panel" aria-labelledby="revisions-title">
          <h2 id="revisions-title">Guideline revision history · latest 100</h2><p>Restoring copies prior instructions into a new revision and retains this history.</p>
          <form className="evolution-form" onSubmit={(event) => { event.preventDefault(); if (restoreNumber.trim() && Number.isInteger(Number(restoreNumber)) && Number(restoreNumber) >= 0 && Number(restoreNumber) < snapshot.active.revision) restoreRevision(Number(restoreNumber)); }}>
            <label className="field" htmlFor="evolution-restore-number">Earlier revision number</label>
            <input id="evolution-restore-number" name="revision" type="number" min={0} max={Math.max(0, snapshot.active.revision - 1)} step={1} value={restoreNumber} onChange={(event) => setRestoreNumber(event.target.value)} />
            <p>You can restore a known revision older than this recent list. Expected active revision: {snapshot.active.revision}.</p>
            <button type="submit" className="button button--secondary" disabled={busy || !restoreNumber.trim() || !Number.isInteger(Number(restoreNumber)) || Number(restoreNumber) < 0 || Number(restoreNumber) >= snapshot.active.revision}>Restore earlier revision</button>
          </form>
          <div className="evolution-list">{snapshot.revisions.map((revision) => <article key={revision.revision} className="evolution-card" aria-label={`Guideline revision ${revision.revision}`}>
            <h3>Revision {revision.revision} · {revision.title}{revision.revision === snapshot.active.revision ? " · Active" : ""}</h3>
            <time dateTime={new Date(revision.createdAtMs).toISOString()}>{dateLabel(revision.createdAtMs)}</time>
            <p>{revision.reason}</p><p className="evolution-text">{revision.instructions || "No additional guidelines."}</p>
            {revision.revision !== snapshot.active.revision && <button type="button" className="button button--secondary" disabled={busy} onClick={() => restoreRevision(revision.revision)}>Restore revision {revision.revision}</button>}
          </article>)}</div>
        </section>
      </>}
    </div>
  </main>;
}
