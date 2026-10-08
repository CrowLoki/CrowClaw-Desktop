import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import { App } from "../App";
import type { CrowClawGateway, EvolutionProposal, ModelConnection } from "../gateway/contracts";
import { createDevelopmentGateway } from "../gateway/developmentGateway";
import { createTauriGateway } from "../gateway/tauriGateway";
import { EvolutionView } from "./EvolutionView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function fixture() {
  const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
  const bootstrap = await gateway.bootstrap();
  const task = (await gateway.sendMessage(bootstrap.selectedConversationId!, "Explain gravity", null)).task;
  const proposal = await gateway.draftEvolution({ title: "Clear explanations", rationale: "Explain assumptions", instructions: "State assumptions before conclusions.", sourceTaskIds: [task.id], baseRevision: 0 });
  return { gateway, connection: bootstrap.connection!, task, proposal };
}

async function openCandidate(proposal: EvolutionProposal) {
  await screen.findByLabelText("Proposal");
  await userEvent.selectOptions(screen.getByLabelText("Proposal"), proposal.id);
}

async function fillManual(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText("Proposal title (required)"), "Clear answers");
  await user.type(screen.getByLabelText("Reason for change (required)"), "Make assumptions visible.");
  await user.type(screen.getByLabelText("Proposed instructions (required)"), "State assumptions.");
}

describe("Governed evolution workflows", () => {
  it("shows the retained reflection goal and feedback instead of later feedback", async () => {
    const {gateway,connection,proposal}=await fixture();
    const snapshot=await gateway.evolutionSnapshot();
    snapshot.proposals[0]={...proposal,model:"requested-alias",reportedModel:"resolved-model",reflectionContext:{goal:"Original reflection goal",selectedEvidence:{feedback:{rating:"needs_improvement",note:"Original feedback A"}}}};
    snapshot.observations[0].feedback={rating:"useful",note:"Later feedback B",updatedAtMs:Date.now()};
    vi.spyOn(gateway,"evolutionSnapshot").mockResolvedValue(snapshot);
    render(<EvolutionView gateway={gateway} connection={connection}/>);
    await openCandidate(proposal);
    await userEvent.click(screen.getByText("Original reflection goal and feedback"));
    expect(screen.getByText("Goal: Original reflection goal")).toBeVisible();
    expect(screen.getByText("Feedback: needs improvement — Original feedback A")).toBeVisible();
    expect(screen.getByText("Requested reflection model: requested-alias")).toBeVisible();
    expect(screen.getByText("Reported reflection model: resolved-model")).toBeVisible();
  });

  it("distinguishes different reported models from the selected comparison alias", async () => {
    const {gateway,connection,proposal}=await fixture();
    await gateway.evaluateEvolution({requestId:"identity-comparison",proposalId:proposal.id,instructions:proposal.instructions,prompt:"Compare reporting"});
    const snapshot=await gateway.evolutionSnapshot();
    snapshot.evaluations[0].baselineModel="reported-A";
    snapshot.evaluations[0].candidateModel="reported-B";
    vi.spyOn(gateway,"evolutionSnapshot").mockResolvedValue(snapshot);
    render(<EvolutionView gateway={gateway} connection={connection}/>);
    expect(await screen.findByText("Reported baseline model: reported-A · Reported candidate model: reported-B")).toBeVisible();
    expect(screen.getByText("The endpoint reported different models for these responses. This comparison does not isolate the guideline change.")).toBeVisible();
  });
  it("has one nav destination, loads only local history on entry and requires an explicit Apply click", async () => {
    const { gateway, proposal } = await fixture();
    const snapshot = vi.spyOn(gateway, "evolutionSnapshot");
    const reflect = vi.spyOn(gateway, "reflectEvolution");
    const evaluate = vi.spyOn(gateway, "evaluateEvolution");
    const decide = vi.spyOn(gateway, "decideEvolution");
    const user = userEvent.setup();
    render(<App gateway={gateway} />);
    const nav = await screen.findAllByRole("button", { name: "Evolution" });
    expect(nav).toHaveLength(1);
    expect(snapshot).not.toHaveBeenCalled();
    await user.click(nav[0]);
    await openCandidate(proposal);
    expect(snapshot).toHaveBeenCalledTimes(1);
    expect(reflect).not.toHaveBeenCalled();
    expect(evaluate).not.toHaveBeenCalled();
    expect(decide).not.toHaveBeenCalled();
    expect(screen.getByText(/A task success is not a quality rating/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Apply proposal" }));
    expect(await screen.findByRole("heading", { name: "Active guidelines · revision 1" })).toBeVisible();
    expect(decide).toHaveBeenCalledWith({ id: proposal.id, decision: "apply", instructions: proposal.instructions, expectedRevision: 0 });
  });

  it("saves and rejects a manual proposal offline without a provider call or activation", async () => {
    const gateway = createDevelopmentGateway({ delayMs: 0 });
    const reflect = vi.spyOn(gateway, "reflectEvolution");
    const evaluate = vi.spyOn(gateway, "evaluateEvolution");
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={null} />);
    await screen.findByLabelText("Proposal title (required)");
    await fillManual(user);
    expect(screen.getByRole("button", { name: "Request model reflection" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Save proposal" }));
    expect(await screen.findByText("Manual proposal · no model used")).toBeVisible();
    expect(screen.getByRole("button", { name: "Compare baseline and candidate" })).toBeDisabled();
    expect((await gateway.evolutionSnapshot()).active.revision).toBe(0);
    await user.click(screen.getByRole("button", { name: "Reject proposal" }));
    await screen.findByText("Proposal rejected. Active guidelines did not change.");
    const snapshot = await gateway.evolutionSnapshot();
    expect(snapshot.proposals[0]).toMatchObject({ status: "rejected", model: null, appliedRevision: null });
    expect(snapshot.revisions).toHaveLength(1);
    expect(reflect).not.toHaveBeenCalled();
    expect(evaluate).not.toHaveBeenCalled();
  });

  it("records explicit quality feedback independently of a successful terminal outcome", async () => {
    const { gateway, connection, task } = await fixture();
    const save = vi.spyOn(gateway, "saveEvolutionFeedback");
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    const card = await screen.findByRole("article", { name: "Task: Explain gravity" });
    expect(within(card).getByText("Outcome: succeeded · Guidelines: revision 0")).toBeVisible();
    expect(within(card).getByRole("button", { name: "Save feedback" })).toBeDisabled();
    expect(within(card).getByRole("radio", { name: "Useful" })).not.toBeChecked();
    await user.click(within(card).getByRole("radio", { name: "Needs improvement" }));
    await user.type(within(card).getByLabelText("Feedback note (optional)"), "Too vague.");
    await user.click(within(card).getByRole("button", { name: "Save feedback" }));
    await screen.findByText("Saved feedback: Needs improvement — Too vague.");
    expect(save).toHaveBeenCalledWith({ taskId: task.id, rating: "needs_improvement", note: "Too vague." });
    expect((await gateway.evolutionSnapshot()).active.revision).toBe(0);
  });

  it.each(["reflection", "comparison"] as const)("cancels an explicitly requested %s using its original request ID", async (kind) => {
    const { gateway, connection, task, proposal } = await fixture();
    const result = deferred<never>();
    const requestSpy = kind === "reflection" ? vi.spyOn(gateway, "reflectEvolution").mockImplementation(() => result.promise) : vi.spyOn(gateway, "evaluateEvolution").mockImplementation(() => result.promise);
    const cancel = vi.spyOn(gateway, "cancelEvolution").mockImplementation(async () => { result.reject(new Error("Model request cancelled.")); });
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    await openCandidate(proposal);
    if (kind === "reflection") {
      await user.selectOptions(screen.getByLabelText("Task to reflect on (required)"), task.id);
      await user.type(screen.getByLabelText("Reflection goal (required)"), "Make answers clearer");
      expect(requestSpy).not.toHaveBeenCalled();
      await user.click(screen.getByRole("button", { name: "Request model reflection" }));
    } else {
      await user.type(screen.getByLabelText("Comparison prompt"), "Why does gravity attract?");
      expect(requestSpy).not.toHaveBeenCalled();
      await user.click(screen.getByRole("button", { name: "Compare baseline and candidate" }));
    }
    const request = requestSpy.mock.calls[0][0];
    expect(request.requestId).toEqual(expect.any(String));
    expect(screen.getByRole("button", { name: "Apply proposal" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Cancel model request" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Model request cancelled.");
    expect(cancel).toHaveBeenCalledWith(request.requestId);
    expect((await gateway.evolutionSnapshot()).active.revision).toBe(0);
    await waitFor(() => expect(screen.queryByRole("button", { name: "Cancel model request" })).not.toBeInTheDocument());
  });

  it("compares typed candidate instructions, records a vote, applies and restores into a new revision", async () => {
    const { gateway, connection, proposal } = await fixture();
    const evaluate = vi.spyOn(gateway, "evaluateEvolution");
    const rate = vi.spyOn(gateway, "rateEvolutionEvaluation");
    const restore = vi.spyOn(gateway, "restoreEvolution");
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} developmentPreview />);
    await openCandidate(proposal);
    await user.clear(screen.getByLabelText("Candidate instructions"));
    await user.type(screen.getByLabelText("Candidate instructions"), "Use one concrete example.");
    await user.type(screen.getByLabelText("Comparison prompt"), "Explain inertia.");
    await user.click(screen.getByRole("button", { name: "Compare baseline and candidate" }));
    const comparison = await screen.findByRole("article", { name: "Comparison: Explain inertia." });
    expect(within(comparison).getByText(/Simulated baseline response to:/)).toBeVisible();
    expect(within(comparison).getByText(/Simulated candidate response to:/)).toBeVisible();
    expect(evaluate).toHaveBeenCalledWith({ requestId: expect.any(String), proposalId: proposal.id, instructions: "Use one concrete example.", prompt: "Explain inertia." });
    await user.click(within(comparison).getByRole("button", { name: "Prefer candidate" }));
    await waitFor(() => expect(within(comparison).getByRole("button", { name: "Prefer candidate" })).toHaveAttribute("aria-pressed", "true"));
    expect(rate).toHaveBeenCalledWith({ id: expect.any(String), preference: "candidate" });
    expect((await gateway.evolutionSnapshot()).active.revision).toBe(0);
    await user.click(screen.getByRole("button", { name: "Apply proposal" }));
    await screen.findByRole("heading", { name: "Active guidelines · revision 1" });
    expect((await gateway.evolutionSnapshot()).active.instructions).toBe("Use one concrete example.");
    await user.click(screen.getByRole("button", { name: "Restore revision 0" }));
    await screen.findByRole("heading", { name: "Active guidelines · revision 2" });
    expect(restore).toHaveBeenCalledWith({ revision: 0, expectedRevision: 1 });
    const history = await gateway.evolutionSnapshot();
    expect(history.active.instructions).toBe("");
    expect(history.revisions.map((item) => item.revision)).toEqual([2, 1, 0]);
    expect(history.evaluations[0].preference).toBe("candidate");
  });

  it("surfaces stale Apply errors, keeps editor text and does not retry or rebase", async () => {
    const { gateway, connection, proposal } = await fixture();
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    await openCandidate(proposal);
    await user.type(screen.getByLabelText("Candidate instructions"), " Keep my edit.");
    const concurrent = await gateway.draftEvolution({ title: "Another change", rationale: "Concurrent decision", instructions: "Concurrent active instructions", baseRevision: 0, sourceTaskIds: [] });
    await gateway.decideEvolution({ id: concurrent.id, decision: "apply", instructions: concurrent.instructions, expectedRevision: 0 });
    const decide = vi.spyOn(gateway, "decideEvolution");
    await user.click(screen.getByRole("button", { name: "Apply proposal" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Stale guideline revision: expected 0, active 1");
    expect(decide).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Candidate instructions")).toHaveValue(`${proposal.instructions} Keep my edit.`);
    await user.click(screen.getByRole("button", { name: "Refresh history" }));
    await screen.findByRole("heading", { name: "Active guidelines · revision 1" });
    expect(screen.getByText(/This draft is based on an earlier revision/)).toBeVisible();
    expect((await gateway.evolutionSnapshot()).active.instructions).toBe(concurrent.instructions);
  });

  it("surfaces a stale Restore error without activating the requested old revision", async () => {
    const { gateway, connection, proposal } = await fixture();
    await gateway.decideEvolution({ id: proposal.id, decision: "apply", instructions: proposal.instructions, expectedRevision: 0 });
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    await screen.findByRole("heading", { name: "Active guidelines · revision 1" });
    await gateway.restoreEvolution({ revision: 0, expectedRevision: 1 });
    const restore = vi.spyOn(gateway, "restoreEvolution");
    await user.click(screen.getByRole("button", { name: "Restore revision 0" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Stale guideline revision: expected 1, active 2");
    expect(restore).toHaveBeenCalledTimes(1);
    expect((await gateway.evolutionSnapshot()).revisions).toHaveLength(3);
  });

  it("keeps newer manual text when a deferred save completes", async () => {
    const gateway = createDevelopmentGateway({ delayMs: 0 });
    const gate = deferred<void>();
    const original = gateway.draftEvolution.bind(gateway);
    vi.spyOn(gateway, "draftEvolution").mockImplementation(async (request) => { await gate.promise; return original(request); });
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={null} />);
    await screen.findByLabelText("Proposal title (required)");
    await fillManual(user);
    await user.click(screen.getByRole("button", { name: "Save proposal" }));
    await user.type(screen.getByLabelText("Proposal title (required)"), " later");
    await user.type(screen.getByLabelText("Reason for change (required)"), " New reason.");
    await user.type(screen.getByLabelText("Proposed instructions (required)"), " New draft.");
    await act(async () => gate.resolve());
    await screen.findByText("Proposal saved for review. It is not active.");
    expect(screen.getByLabelText("Proposal title (required)")).toHaveValue("Clear answers later");
    expect(screen.getByLabelText("Reason for change (required)")).toHaveValue("Make assumptions visible. New reason.");
    expect(screen.getByLabelText("Proposed instructions (required)")).toHaveValue("State assumptions. New draft.");
    expect((await gateway.evolutionSnapshot()).proposals[0].instructions).toBe("State assumptions.");
  });

  it("keeps newer candidate and goal edits when a reflection finishes", async () => {
    const { gateway, connection, proposal, task } = await fixture();
    const gate = deferred<void>();
    const original = gateway.reflectEvolution.bind(gateway);
    vi.spyOn(gateway, "reflectEvolution").mockImplementation(async (request) => { await gate.promise; return original(request); });
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    await openCandidate(proposal);
    await user.selectOptions(screen.getByLabelText("Task to reflect on (required)"), task.id);
    await user.type(screen.getByLabelText("Reflection goal (required)"), "Be precise.");
    await user.click(screen.getByRole("button", { name: "Request model reflection" }));
    await user.type(screen.getByLabelText("Candidate instructions"), " Keep newer text.");
    await user.type(screen.getByLabelText("Reflection goal (required)"), " New goal.");
    await act(async () => gate.resolve());
    await screen.findByText("Reflection proposal saved for review. It is not active.");
    expect(screen.getByLabelText("Candidate instructions")).toHaveValue(`${proposal.instructions} Keep newer text.`);
    expect(screen.getByLabelText("Proposal")).toHaveValue(proposal.id);
    expect(screen.getByLabelText("Reflection goal (required)")).toHaveValue("Be precise. New goal.");
    expect((await gateway.evolutionSnapshot()).proposals[0]).toMatchObject({ model: connection.model, instructions: "Simulated candidate guideline: Be precise." });
  });

  it("preserves typed comparison edits and records only the submitted candidate and prompt", async () => {
    const { gateway, connection, proposal } = await fixture();
    const gate = deferred<void>();
    const original = gateway.evaluateEvolution.bind(gateway);
    vi.spyOn(gateway, "evaluateEvolution").mockImplementation(async (request) => { await gate.promise; return original(request); });
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    await openCandidate(proposal);
    await user.type(screen.getByLabelText("Comparison prompt"), "Explain force.");
    await user.click(screen.getByRole("button", { name: "Compare baseline and candidate" }));
    await user.type(screen.getByLabelText("Candidate instructions"), " Newer edit.");
    await user.type(screen.getByLabelText("Comparison prompt"), " Newer prompt.");
    await act(async () => gate.resolve());
    await screen.findByRole("article", { name: "Comparison: Explain force." });
    expect(screen.getByLabelText("Candidate instructions")).toHaveValue(`${proposal.instructions} Newer edit.`);
    expect(screen.getByLabelText("Comparison prompt")).toHaveValue("Explain force. Newer prompt.");
    expect(screen.getByText("The current editor differs from this evaluated candidate.")).toBeVisible();
    expect((await gateway.evolutionSnapshot()).evaluations[0]).toMatchObject({ candidateInstructions: proposal.instructions, prompt: "Explain force." });
  });

  it("applies submitted text while preserving newer edits made during the decision", async () => {
    const { gateway, connection, proposal } = await fixture();
    const gate = deferred<void>();
    const original = gateway.decideEvolution.bind(gateway);
    vi.spyOn(gateway, "decideEvolution").mockImplementation(async (request) => { await gate.promise; return original(request); });
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    await openCandidate(proposal);
    await user.click(screen.getByRole("button", { name: "Apply proposal" }));
    await user.type(screen.getByLabelText("Candidate instructions"), " Future draft.");
    await act(async () => gate.resolve());
    await screen.findByRole("heading", { name: "Active guidelines · revision 1" });
    expect(screen.getByLabelText("Candidate instructions")).toHaveValue(`${proposal.instructions} Future draft.`);
    expect(screen.getByText(/Newer editor text was kept; it was not applied/)).toBeVisible();
    expect((await gateway.evolutionSnapshot()).active.instructions).toBe(proposal.instructions);
  });

  it("retains the recorded reflection and comparison model after the live connection changes", async () => {
    const { gateway, connection, task } = await fixture();
    const reflected = await gateway.reflectEvolution({ requestId: "recorded-reflection", taskId: task.id, goal: "Use examples." });
    await gateway.evaluateEvolution({ requestId: "recorded-evaluation", proposalId: reflected.id, instructions: reflected.instructions, prompt: "Explain momentum." });
    const switched: ModelConnection = await gateway.connectModel({ provider: "ollama", label: "Changed connection", baseUrl: "http://127.0.0.1:11434/v1", model: "different-model" });
    render(<EvolutionView gateway={gateway} connection={switched} />);
    await openCandidate(reflected);
    expect(screen.getByText(`Requested reflection model: ${connection.model}`)).toBeVisible();
    expect(screen.getByText(`Requested comparison model: ${connection.model}`)).toBeVisible();
    expect(screen.queryByText(/Requested reflection model: different-model|Requested comparison model: different-model/)).not.toBeInTheDocument();
  });

  it("reports a local snapshot error and recovers using explicit refresh", async () => {
    const { gateway, connection } = await fixture();
    vi.spyOn(gateway, "evolutionSnapshot").mockRejectedValueOnce(new Error("History database busy"));
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("History database busy");
    await user.click(screen.getByRole("button", { name: "Refresh history" }));
    await screen.findByRole("heading", { name: "Active guidelines · revision 0" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("Evolution adapter boundaries", () => {
  it("sends all eight mutation requests in their native envelopes and reads the snapshot locally", async () => {
    const gateway = createTauriGateway();
    const mock = vi.mocked(invoke);
    mock.mockReset(); mock.mockResolvedValue(undefined);
    const draft = { title: "Proposal", rationale: "Reason", instructions: "Guideline", sourceTaskIds: [], baseRevision: 3 };
    const requests: Array<[keyof CrowClawGateway, string, unknown[] , unknown]> = [
      ["evolutionSnapshot", "snapshot", [], undefined],
      ["saveEvolutionFeedback", "feedback", [{ taskId: "t1", rating: "uncertain", note: "Check facts" }], { request: { taskId: "t1", rating: "uncertain", note: "Check facts" } }],
      ["draftEvolution", "draft", [draft], { request: draft }],
      ["reflectEvolution", "reflect", [{ requestId: "r1", taskId: "t1", goal: "Improve clarity" }], { request: { requestId: "r1", taskId: "t1", goal: "Improve clarity" } }],
      ["evaluateEvolution", "evaluate", [{ requestId: "r2", proposalId: "p1", instructions: "Guideline", prompt: "Example" }], { request: { requestId: "r2", proposalId: "p1", instructions: "Guideline", prompt: "Example" } }],
      ["rateEvolutionEvaluation", "rate", [{ id: "e1", preference: "tie" }], { request: { id: "e1", preference: "tie" } }],
      ["decideEvolution", "decide", [{ id: "p1", decision: "apply", instructions: "Guideline", expectedRevision: 3 }], { request: { id: "p1", decision: "apply", instructions: "Guideline", expectedRevision: 3 } }],
      ["restoreEvolution", "restore", [{ revision: 2, expectedRevision: 4 }], { request: { revision: 2, expectedRevision: 4 } }],
      ["cancelEvolution", "cancel", ["r2"], { request: { requestId: "r2" } }],
    ];
    for (const [method, suffix, args, expected] of requests) {
      await (gateway[method] as (...values: unknown[]) => Promise<unknown>)(...args);
      expect(mock).toHaveBeenLastCalledWith(`crowclaw_evolution_${suffix}`, expected);
    }
    mock.mockRejectedValueOnce("Stale revision from native storage");
    await expect(gateway.restoreEvolution({ revision: 2, expectedRevision: 4 })).rejects.toThrow("Stale revision from native storage");
  });

  it("cancels simulated work before it creates a proposal or evaluation", async () => {
    vi.useFakeTimers();
    try {
      const gateway = createDevelopmentGateway({ firstRun: false, includeRunningTask: true, delayMs: 10 });
      const terminal = gateway.cancelTask("task-running");
      await vi.advanceTimersByTimeAsync(10);
      const task = (await terminal).task;
      const work = gateway.reflectEvolution({ requestId: "simulated-cancel", taskId: task.id, goal: "Clarify" });
      const outcome = expect(work).rejects.toThrow("Model request cancelled.");
      await gateway.cancelEvolution("simulated-cancel");
      await vi.advanceTimersByTimeAsync(10);
      await outcome;
      const read = gateway.evolutionSnapshot();
      await vi.advanceTimersByTimeAsync(10);
      expect((await read).proposals).toHaveLength(0);
    } finally { vi.useRealTimers(); }
  });

  it("limits recent history but restores a known revision older than the displayed window", async () => {
    const { gateway, connection, proposal } = await fixture();
    await gateway.decideEvolution({ id: proposal.id, decision: "apply", instructions: proposal.instructions, expectedRevision: 0 });
    for (let revision = 1; revision <= 100; revision++) await gateway.restoreEvolution({ revision: 0, expectedRevision: revision });
    const snapshot = await gateway.evolutionSnapshot();
    expect(snapshot.revisions).toHaveLength(100);
    expect(snapshot.revisions.some((item) => item.revision === 0)).toBe(false);
    const restore = vi.spyOn(gateway, "restoreEvolution");
    const user = userEvent.setup();
    render(<EvolutionView gateway={gateway} connection={connection} />);
    await screen.findByRole("heading", { name: "Active guidelines · revision 101" });
    await user.type(screen.getByLabelText("Earlier revision number"), "0");
    await user.click(screen.getByRole("button", { name: "Restore earlier revision" }));
    await screen.findByRole("heading", { name: "Active guidelines · revision 102" });
    expect(restore).toHaveBeenCalledWith({ revision: 0, expectedRevision: 101 });
  });
});
