import type { ConversationComposerSnapshot, ConversationComposerState, ComposerModelSource } from './composerContracts';
import type { AttachmentPreview, AttachmentSummary } from './attachmentContracts';
import type {
  ActionDecision,
  ActionDecisionResult,
  AgentTask,
  AppBootstrap,
  AppSettings,
  ChatTurnResult,
  ConnectionTestResult,
  Conversation,
  ConversationMessage,
  ConversationSummary,
  CrowClawGateway,
  CrowQuantMemory,
  CrowQuantSearchHit,
  DiscoveredEndpoint,
  MemoryRecord,
  ModelConnection,
  ModelEndpointDraft,
  PendingAction,
  SelectedFolder,
  TaskCancellationResult,
  MemorySettings, MemoryQuery, NativeMemoryHit,
  EvolutionSnapshot, EvolutionProposal, EvolutionRevision, EvolutionDraftRequest, EvolutionObservation,
} from "./contracts";

type DevelopmentGatewayOptions = {
  firstRun?: boolean;
  includeRunningTask?: boolean;
  delayMs?: number;
};

const defaultSettings: AppSettings = {
  permissions: {
    readFiles: "ask",
    writeFiles: "ask",
    runCommands: "ask",
  },
  launchAtLogin: false,
  keepRunningOnClose: true,
  retainConversations: true,
  theme: "dark",
};

const discoveredEndpoints: DiscoveredEndpoint[] = [
  {
    id: "detected-lm-studio",
    provider: "lm-studio",
    label: "LM Studio",
    baseUrl: "http://127.0.0.1:1234/v1",
    model: "local-model",
    detected: true,
    availableModels: ["local-model"],
  },
  {
    id: "detected-ollama",
    provider: "ollama",
    label: "Ollama",
    baseUrl: "http://127.0.0.1:11434/v1",
    model: "qwen3.5:9b",
    detected: true,
    availableModels: ["qwen3.5:9b", "gemma3:4b"],
  },
];

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

async function nativeMembershipOnly(): Promise<never> {
  throw new Error("ChatGPT sign-in and account operations require the installed native CrowClaw application. They are unsupported in the development preview.");
}

function now(): string {
  return new Date().toISOString();
}

function createId(prefix: string, counter: number): string {
  return `${prefix}-${counter.toString().padStart(4, "0")}`;
}

function searchTerms(value: string): Set<string> {
  return new Set(value.toLocaleLowerCase().match(/[a-z0-9]+/g) ?? []);
}

function similarityScore(query: string, text: string): number {
  const queryTerms = searchTerms(query);
  const textTerms = searchTerms(text);
  if (queryTerms.size === 0 || textTerms.size === 0) return 0;
  let overlap = 0;
  queryTerms.forEach((term) => {
    if (textTerms.has(term)) overlap += 1;
  });
  return overlap / Math.sqrt(queryTerms.size * textTerms.size);
}

function summaryFor(conversation: Conversation): ConversationSummary {
  const latest = conversation.messages.at(-1);
  return {
    id: conversation.id,
    title: conversation.title,
    preview: latest?.content ?? "No messages yet",
    updatedAt: conversation.updatedAt,
    unread: false,
  };
}

function defaultConversation(): Conversation {
  const timestamp = now();
  return {
    id: "conversation-welcome",
    title: "Welcome to CrowClaw",
    createdAt: timestamp,
    updatedAt: timestamp,
    messages: [
      {
        id: "message-welcome",
        role: "assistant",
        content:
          "I’m ready when your local model is connected. Ask a question, give me a task, or request a file action—I’ll show you exactly what needs approval before it runs.",
        createdAt: timestamp,
        status: "sent",
      },
    ],
  };
}

export function createDevelopmentGateway(
  options: DevelopmentGatewayOptions = {},
): CrowClawGateway {
  let counter = 10;
  let firstRun = options.firstRun ?? true;
  let settings = clone(defaultSettings);
  let memorySettings: MemorySettings = { indexConversations: true, indexActions: false };
  const withdrawnSources = new Set<string>();
  let connection: ModelConnection | null = firstRun
    ? null
    : {
        id: "connection-development",
        provider: "lm-studio",
        label: "LM Studio",
        baseUrl: "http://127.0.0.1:1234/v1",
        model: "local-model",
        status: "connected",
        connectedAt: now(),
        latencyMs: 18,
      };
  const welcome = defaultConversation();
  const conversations = new Map<string, Conversation>([[welcome.id, welcome]]);
  const composerConnections = new Map<string,ModelConnection>(connection ? [[connection.id,clone(connection)]] : []);
  const composers = new Map<string,ConversationComposerState>();
  // Immutable, session-only UX fixture. No picker, filesystem or provider access.
  const attachmentPreviews = new Map<string, AttachmentPreview>();
  function composerState(id:string):ConversationComposerState {
    requireConversation(id);
    let saved=composers.get(id);
    if (!saved) {
      saved={conversationId:id,revision:1,draft:'',attachments:[],selection:connection?{providerProfileId:connection.id,model:connection.model,reasoningEffort:null}:null};
      composers.set(id,saved);
    }
    return saved;
  }
  function composerSources():ComposerModelSource[] {
    return [...composerConnections.values()].map(profile=>({id:profile.id,label:profile.label,provider:profile.provider,status:profile.status==='connected'?'ready':'disconnected',models:[...new Set([profile.model,...(discoveredEndpoints.find(endpoint=>endpoint.baseUrl===profile.baseUrl)?.availableModels??[])])].map(id=>({id,displayName:id,reasoningEfforts:[]}))}));
  }
  function composerSnapshot(id:string):ConversationComposerSnapshot {
    const saved=composerState(id), profile=saved.selection?composerConnections.get(saved.selection.providerProfileId):undefined;
    return clone({composer:saved,hiddenModelKeys,connection:profile&&saved.selection?{...profile,model:saved.selection.model}:null,sources:composerSources(),warning:profile?null:'Choose a connection and model for this conversation'});
  }
  function requireComposerRevision(id:string,revision:number) {
    const saved=composerState(id);
    if(saved.revision!==revision)throw new Error('This conversation changed; refresh its composer.');
    return saved;
  }
  let hiddenModelKeys:string[]=[];
  let tasks: AgentTask[] = options.includeRunningTask
    ? [
        {
          id: "task-running",
          conversationId: welcome.id,
          title: "Review selected notes",
          detail: "Preparing a summary from approved local files",
          status: "running",
          progress: 42,
          startedAt: now(),
          updatedAt: now(),
          cancellable: true,
        },
      ]
    : [];
  let pendingActions: PendingAction[] = [];
  let memories: MemoryRecord[] = [
    {
      id: "memory-local-first",
      title: "Local model preference",
      preview: "Use the connected local model by default and ask before reading files.",
      source: "user-note",
      conversationId: null,
      createdAt: now(),
      tags: ["local", "permissions"],
    },
  ];
  let crowQuantMemories: CrowQuantMemory[] = [];
  // Session-only simulation. No provider call, native persistence or quality claim.
  const initialRevision: EvolutionRevision = { revision: 0, title: "Initial guidelines", instructions: "", reason: "Development simulation baseline", createdAtMs: Date.now() };
  const evolution: EvolutionSnapshot = { active: initialRevision, observations: [], proposals: [], revisions: [initialRevision], evaluations: [] };
  const taskRevisions = new Map<string, number>();
  const evolutionRequests = new Map<string, { cancelled: boolean }>();

  function checkRevision(expected: number) {
    if (expected !== evolution.active.revision) throw new Error(`Stale guideline revision: expected ${expected}, active ${evolution.active.revision}. Refresh and review before trying again.`);
  }

  function requireProposal(id: string) {
    const proposal = evolution.proposals.find((value) => value.id === id);
    if (!proposal) throw new Error("That proposal is no longer available.");
    return proposal;
  }

  function addDraft(request: EvolutionDraftRequest, model: string | null = null): EvolutionProposal {
    checkRevision(request.baseRevision);
    if (!request.title.trim() || !request.rationale.trim() || !request.instructions.trim()) throw new Error("Enter a title, reason and candidate instructions.");
    if (request.sourceTaskIds.some((id) => !evolution.observations.some((item) => item.taskId === id))) throw new Error("Choose a retained terminal task.");
    const proposal: EvolutionProposal = { ...clone(request), model, reportedModel: null, reflectionContext: null, id: createId("proposal", ++counter), status: "draft", createdAtMs: Date.now(), decidedAtMs: null, appliedRevision: null };
    evolution.proposals.unshift(proposal);
    return clone(proposal);
  }

  async function simulateModel<T>(requestId: string, work: () => T): Promise<T> {
    if (!connection || connection.status !== "connected") throw new Error("Connect a model before requesting reflection or comparison.");
    if (!requestId || evolutionRequests.has(requestId)) throw new Error("Model request ID must be unique.");
    const token = { cancelled: false };
    evolutionRequests.set(requestId, token);
    try {
      await pause();
      if (token.cancelled) throw new Error("Model request cancelled.");
      return work();
    } finally { evolutionRequests.delete(requestId); }
  }

  function addRevision(title: string, instructions: string, reason: string): EvolutionRevision {
    const revision = { revision: evolution.active.revision + 1, title, instructions, reason, createdAtMs: Date.now() };
    evolution.active = revision;
    evolution.revisions.unshift(revision);
    return clone(revision);
  }

  async function pause(): Promise<void> {
    if ((options.delayMs ?? 90) <= 0) return;
    await new Promise((resolve) => window.setTimeout(resolve, options.delayMs ?? 90));
  }

  function requireConversation(conversationId: string): Conversation {
    const conversation = conversations.get(conversationId);
    if (!conversation) throw new Error("That conversation is no longer available.");
    return conversation;
  }

  function replaceTask(task: AgentTask): void {
    tasks = [task, ...tasks.filter(({ id }) => id !== task.id)];
    if (["completed", "failed", "cancelled"].includes(task.status)) {
      const feedback = evolution.observations.find((item) => item.taskId === task.id)?.feedback ?? null;
      const observation: EvolutionObservation = { taskId: task.id, title: task.title, outcome: task.status === "completed" ? "succeeded" : task.status === "failed" ? "failed" : "cancelled", guidelineRevision: taskRevisions.get(task.id) ?? null, updatedAtMs: Date.parse(task.updatedAt), feedback };
      evolution.observations = [observation, ...evolution.observations.filter((item) => item.taskId !== task.id)].slice(0, 50);
    }
  }

  return {
    async getComposer(id) {return composerSnapshot(id);},
    async selectAttachments(id, revision) {
      await pause();
      const saved = requireComposerRevision(id, revision);
      const attachments = saved.attachments ?? [];
      if (attachments.length >= 8) throw new Error('A draft can contain at most 8 files. Remove a file and retry.');
      const attachment: AttachmentSummary = {
        id: createId('fixture-attachment', ++counter), conversationId: id, messageId: null,
        name: 'Development fixture.txt', mediaType: 'text/plain', kind: 'text', byteLength: 32,
        sha256: 'ba4010c30b99298ffb2f241cb48de3ac7ac7b70a7fa1c290f6dc86b93a03af57', createdAtMs: Date.now(),
      };
      attachmentPreviews.set(attachment.id, { attachment: clone(attachment), text: 'Development attachment fixture.\n', dataUrl: null });
      composers.set(id, { ...saved, attachments: [...attachments, attachment], revision: revision + 1 });
      return composerSnapshot(id);
    },
    async removeAttachment(id, revision, attachmentId) {
      await pause();
      const saved = requireComposerRevision(id, revision);
      const attachments = saved.attachments ?? [];
      if (!attachments.some(item => item.id === attachmentId)) throw new Error('That attachment is not in this draft. Refresh the composer.');
      composers.set(id, { ...saved, attachments: attachments.filter(item => item.id !== attachmentId), revision: revision + 1 });
      attachmentPreviews.delete(attachmentId);
      return composerSnapshot(id);
    },
    async previewAttachment(id, attachmentId) {
      await pause();
      requireConversation(id);
      const preview = attachmentPreviews.get(attachmentId);
      if (!preview || preview.attachment.conversationId !== id) throw new Error('That attachment is unavailable in this conversation.');
      return clone(preview);
    },
    async saveComposerDraft(id,revision,draft) {
      const saved=requireComposerRevision(id,revision);
      composers.set(id,{...saved,draft,revision:revision+1});return composerSnapshot(id);
    },
    async chooseComposerModel(id,revision,selection) {
      const saved=requireComposerRevision(id,revision),source=composerSources().find(source=>source.id===selection.providerProfileId);
      if(source?.status!=='ready'||!source.models.some(model=>model.id===selection.model)||selection.reasoningEffort!==null)throw new Error('Choose an available preview model.');
      composers.set(id,{...saved,selection:clone(selection),revision:revision+1});return composerSnapshot(id);
    },
    async refreshComposerModels(id) {const source=composerSources().find(source=>source.id===id);if(!source)throw new Error('Connection was not found.');return clone(source);},
    async setHiddenModels(keys) {hiddenModelKeys=[...keys];return [...hiddenModelKeys];},
    async evolutionSnapshot() { await pause(); return clone({ ...evolution, proposals: evolution.proposals.slice(0, 100), revisions: evolution.revisions.slice(0, 100), evaluations: evolution.evaluations.slice(0, 100) }); },
    async membershipSnapshot() { return { accounts: [], welcomeAcknowledged: false }; },
    signInMembership: nativeMembershipOnly,
    cancelMembershipSignIn: nativeMembershipOnly,
    signOutMembership: nativeMembershipOnly,
    refreshMembershipModels: nativeMembershipOnly,
    useMembershipModel: nativeMembershipOnly,
    acknowledgeMembershipWelcome: nativeMembershipOnly,
    manageMembershipUsage: nativeMembershipOnly,
    beginCodexImageAuthorization: nativeMembershipOnly,
    pollCodexImageAuthorization: nativeMembershipOnly,
    async codexImageAuthorizationStatus() { return { state: "not_connected", verificationUrl: null, userCode: null, pollIntervalSeconds: null, message: null }; },
    cancelCodexImageAuthorization: nativeMembershipOnly,
    async saveEvolutionFeedback(request) {
      await pause();
      const observation = evolution.observations.find((item) => item.taskId === request.taskId);
      if (!observation) throw new Error("That terminal task is no longer available.");
      observation.feedback = { rating: request.rating, note: request.note, updatedAtMs: Date.now() };
    },
    async draftEvolution(request) { await pause(); return addDraft(request); },
    async reflectEvolution(request) {
      const baseRevision = evolution.active.revision;
      const model = connection?.model ?? "";
      return simulateModel(request.requestId, () => {
        if (!request.goal.trim()) throw new Error("Enter a reflection goal.");
        const task = evolution.observations.find((item) => item.taskId === request.taskId);
        if (!task) throw new Error("Choose a retained terminal task.");
        return addDraft({ title: "Simulated reflection", rationale: `Development simulation for ${task.title}. No model was called and no improvement is proven.`, instructions: `Simulated candidate guideline: ${request.goal.trim()}`, sourceTaskIds: [task.taskId], baseRevision }, model);
      });
    },
    async evaluateEvolution(request) {
      const baseline = clone(evolution.active);
      const model = connection?.model ?? "";
      return simulateModel(request.requestId, () => {
        checkRevision(baseline.revision);
        const proposal = requireProposal(request.proposalId);
        if (proposal.status !== "draft") throw new Error("Only a draft proposal can be compared.");
        checkRevision(proposal.baseRevision);
        if (!request.instructions.trim() || !request.prompt.trim()) throw new Error("Enter candidate instructions and a comparison prompt.");
        const evaluation = { id: createId("evaluation", ++counter), proposalId: proposal.id, model, baselineModel:null,candidateModel:null,baselineRevision: baseline.revision, candidateInstructions: request.instructions, prompt: request.prompt, baselineResponse: `Simulated baseline response to: ${request.prompt}\nGuidelines: ${baseline.instructions || "No additional guidelines"}`, candidateResponse: `Simulated candidate response to: ${request.prompt}\nGuidelines: ${request.instructions}`, preference: null, createdAtMs: Date.now() };
        evolution.evaluations.unshift(evaluation);
        return clone(evaluation);
      });
    },
    async rateEvolutionEvaluation(request) {
      await pause();
      const evaluation = evolution.evaluations.find((item) => item.id === request.id);
      if (!evaluation) throw new Error("That comparison is no longer available.");
      evaluation.preference = request.preference;
    },
    async decideEvolution(request) {
      await pause();
      checkRevision(request.expectedRevision);
      const proposal = requireProposal(request.id);
      if (proposal.status !== "draft") throw new Error("That proposal has already been decided.");
      if (request.decision === "apply") {
        checkRevision(proposal.baseRevision);
        if (!request.instructions.trim()) throw new Error("Enter candidate instructions before applying.");
        proposal.appliedRevision = addRevision(proposal.title, request.instructions, proposal.rationale).revision;
      }
      proposal.instructions = request.instructions;
      proposal.status = request.decision === "apply" ? "applied" : "rejected";
      proposal.decidedAtMs = Date.now();
      return clone(proposal);
    },
    async restoreEvolution(request) {
      await pause();
      checkRevision(request.expectedRevision);
      const previous = evolution.revisions.find((item) => item.revision === request.revision);
      if (!previous) throw new Error("That guideline revision is no longer available.");
      if (previous.revision === evolution.active.revision) throw new Error("That guideline revision is already active.");
      return addRevision(previous.title, previous.instructions, `Restored revision ${previous.revision}.`);
    },
    async cancelEvolution(requestId) {
      const token = evolutionRequests.get(requestId);
      if (!token) throw new Error("That model request is no longer running.");
      token.cancelled = true;
    },
    async openRouterCatalog() {
      throw new Error('Live OpenRouter catalog requires the installed desktop app. Browser preview does not access OpenRouter.');
    },
    async connectOpenRouter() {
      throw new Error('OpenRouter key storage requires the installed desktop app. No key was saved in this browser.');
    },
    async disconnectOpenRouter() {
      throw new Error('OpenRouter disconnection requires the installed desktop app.');
    },
    async bootstrap(): Promise<AppBootstrap> {
      await pause();
      const summaries = [...conversations.values()]
        .map(summaryFor)
        .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
      return clone({
        firstRun,
        connection,
        conversations: summaries,
        selectedConversationId: summaries[0]?.id ?? null,
        tasks,
        pendingActions,
        memories,
        settings,
      });
    },

    async discoverEndpoints(): Promise<DiscoveredEndpoint[]> {
      await pause();
      return clone(discoveredEndpoints);
    },

    async testConnection(draft: ModelEndpointDraft): Promise<ConnectionTestResult> {
      await pause();
      let parsed: URL;
      try {
        parsed = new URL(draft.baseUrl);
      } catch {
        return { ok: false, latencyMs: null, resolvedModel: null, detail: "Enter a valid HTTP endpoint." };
      }
      if (!['http:', 'https:'].includes(parsed.protocol)) {
        return { ok: false, latencyMs: null, resolvedModel: null, detail: "The endpoint must use HTTP or HTTPS." };
      }
      if (!draft.model.trim()) {
        return { ok: false, latencyMs: null, resolvedModel: null, detail: "Choose or enter a model name." };
      }
      return {
        ok: true,
        latencyMs: 18,
        resolvedModel: draft.model.trim(),
        detail: `Connected to ${draft.label || "local endpoint"}.`,
      };
    },

    async connectModel(draft: ModelEndpointDraft): Promise<ModelConnection> {
      const tested = await this.testConnection(draft);
      if (!tested.ok) throw new Error(tested.detail);
      const added:ModelConnection = {
        id: createId("connection", ++counter),
        provider: draft.provider,
        label: draft.label.trim() || "Local endpoint",
        baseUrl: draft.baseUrl.trim(),
        model: draft.model.trim(),
        status: "connected",
        connectedAt: now(),
        latencyMs: tested.latencyMs,
      };
      if (draft.provider !== 'crowbot-ai' || firstRun) connection=added;
      firstRun = false;
      composerConnections.set(added.id,clone(added));
      return clone(added);
    },

    async createConversation(): Promise<{ conversation: Conversation; summary: ConversationSummary }> {
      await pause();
      const timestamp = now();
      const conversation: Conversation = {
        id: createId("conversation", ++counter),
        title: "New conversation",
        createdAt: timestamp,
        updatedAt: timestamp,
        messages: [],
      };
      conversations.set(conversation.id, conversation);
      return clone({ conversation, summary: summaryFor(conversation) });
    },

    async getConversation(conversationId: string): Promise<Conversation> {
      await pause();
      return clone(requireConversation(conversationId));
    },

    async selectFolder(): Promise<SelectedFolder> {
      await pause();
      return {
        id: "development-selected-folder",
        name: "Selected notes",
        displayPath: "Development preview folder",
      };
    },

    async sendMessage(
      conversationId: string,
      content: string,
      selectedFolder: SelectedFolder | null,
      composerRevision?: number,
    ): Promise<ChatTurnResult> {
      await pause();
      const savedComposer=composerState(conversationId);
      if(composerRevision!==undefined)requireComposerRevision(conversationId,composerRevision);
      if(composerRevision!==undefined && savedComposer.draft.trim()!==content.trim() && !(savedComposer.draft.trim()==='' && selectedFolder))throw new Error('The draft changed before submission; review the current text and send again.');
      const chosenConnection=composerSnapshot(conversationId).connection;
      if (!chosenConnection || chosenConnection.status !== "connected") {
        throw new Error("Connect a local model before sending a message.");
      }
      const conversation = requireConversation(conversationId);
      const attachments = savedComposer.attachments ?? [];
      if (!content.trim() && !selectedFolder && attachments.length === 0) throw new Error('Write a message or select a file or folder before sending.');
      const requestAttachments = [...conversation.messages.flatMap(message => message.attachments ?? []), ...attachments];
      if (requestAttachments.length > 8) throw new Error('This conversation exceeds eight attachments in a model request; start a new conversation for more files.');
      if (requestAttachments.reduce((total, attachment) => total + attachment.byteLength, 0) > 40 * 1024 * 1024) throw new Error('Attachments in this conversation exceed the 40 MiB request limit; start a new conversation.');
      composers.set(conversationId,{...savedComposer,draft:'',attachments:[],revision:savedComposer.revision+1});
      const timestamp = now();
      const userMessage: ConversationMessage = {
        id: createId("message", ++counter),
        role: "user",
        content: content.trim(),
        createdAt: timestamp,
        status: "sent",
      };
      userMessage.attachments = attachments.map(attachment => ({ ...attachment, messageId: userMessage.id }));
      for (const attachment of userMessage.attachments) {
        const preview = attachmentPreviews.get(attachment.id)!;
        attachmentPreviews.set(attachment.id, { ...preview, attachment: clone(attachment) });
      }
      const firstUserMessage = !conversation.messages.some(({ role }) => role === "user");
      if (firstUserMessage) {
        conversation.title = content.trim().slice(0, 42) || "New conversation";
      }
      conversation.messages.push(userMessage);
      conversation.updatedAt = timestamp;

      const task: AgentTask = {
        id: createId("task", ++counter),
        conversationId,
        title: content.trim().slice(0, 54) || "CrowClaw task",
        detail: "Working with the connected local model",
        status: "running",
        progress: null,
        startedAt: timestamp,
        updatedAt: timestamp,
        cancellable: true,
      };
      taskRevisions.set(task.id, evolution.active.revision);

      const requestsFiles =
        selectedFolder !== null || /\b(inspect|folder|file|read|summari[sz]e)\b/i.test(content);
      let pendingAction: PendingAction | null = null;
      if (requestsFiles) {
        pendingAction = {
          id: createId("action", ++counter),
          taskId: task.id,
          conversationId,
          kind: "read-files",
          title: "Read text files in a selected folder",
          summary: "CrowClaw wants to list text files and read only the file you approve.",
          target: `${selectedFolder?.name ?? "User-selected folder"} · *.txt`,
          details: [
            "List file names ending in .txt",
            "Do not open file contents until you approve a specific read",
            "Keep access limited to the selected folder",
          ],
          risk: "low",
          requestedAt: now(),
        };
        task.status = "waiting-approval";
        task.detail = "Waiting for your file-read decision";
        const assistantMessage: ConversationMessage = {
          id: createId("message", ++counter),
          role: "assistant",
          content: "I’ve prepared a local file-read request. Nothing will be read until you approve it.",
          createdAt: now(),
          status: "waiting-approval",
          taskId: task.id,
        };
        conversation.messages.push(assistantMessage);
        pendingActions = [pendingAction, ...pendingActions];
      } else {
        task.status = "completed";
        task.progress = 100;
        task.cancellable = false;
        task.detail = "Local response completed";
        conversation.messages.push({
          id: createId("message", ++counter),
          role: "assistant",
          content: `I’m connected through ${chosenConnection.label} using ${chosenConnection.model}. This development preview confirms the desktop conversation flow; native model streaming is supplied by the Tauri runtime.`,
          createdAt: now(),
          status: "sent",
          taskId: task.id,
        });
      }
      conversation.updatedAt = now();
      conversations.set(conversation.id, conversation);
      replaceTask(task);
      return clone({
        conversation,
        summary: summaryFor(conversation),
        task,
        pendingActions: pendingAction ? [pendingAction] : [],
      });
    },

    async cancelTask(taskId: string): Promise<TaskCancellationResult> {
      await pause();
      const existing = tasks.find(({ id }) => id === taskId);
      if (!existing) throw new Error("That task is no longer available.");
      if (!existing.cancellable) throw new Error("That task has already finished.");
      const task: AgentTask = {
        ...existing,
        status: "cancelled",
        detail: "Cancelled by you",
        progress: null,
        updatedAt: now(),
        cancellable: false,
      };
      replaceTask(task);
      pendingActions = pendingActions.filter(({ taskId: owner }) => owner !== taskId);
      const conversation = conversations.get(task.conversationId) ?? null;
      if (conversation) {
        conversation.messages.push({
          id: createId("message", ++counter),
          role: "assistant",
          content: "Task cancelled. No further action was taken.",
          createdAt: now(),
          status: "sent",
          taskId,
        });
        conversation.updatedAt = now();
      }
      return clone({
        task,
        conversation,
        summary: conversation ? summaryFor(conversation) : null,
      });
    },

    async decideAction(actionId: string, decision: ActionDecision): Promise<ActionDecisionResult> {
      await pause();
      const action = pendingActions.find(({ id }) => id === actionId);
      if (!action) throw new Error("That action request is no longer pending.");
      pendingActions = pendingActions.filter(({ id }) => id !== actionId);
      const conversation = requireConversation(action.conversationId);
      const existingTask = tasks.find(({ id }) => id === action.taskId);
      if (!existingTask) throw new Error("The action’s task is no longer available.");
      const approved = decision === "approved";
      const task: AgentTask = {
        ...existingTask,
        status: approved ? "completed" : "cancelled",
        detail: approved ? "Approved action recorded" : "Action denied by you",
        progress: approved ? 100 : null,
        updatedAt: now(),
        cancellable: false,
      };
      replaceTask(task);
      conversation.messages = conversation.messages.map((message) =>
        message.taskId === task.id && message.status === "waiting-approval"
          ? { ...message, status: "sent" }
          : message,
      );
      conversation.messages.push({
        id: createId("message", ++counter),
        role: "assistant",
        content: approved
          ? "You approved the scoped read. In this clearly labelled development adapter, no file is actually opened; the native runtime will execute and persist approved actions."
          : "You denied the file read. I did not access the folder or its contents.",
        createdAt: now(),
        status: "sent",
        taskId: task.id,
      });
      conversation.updatedAt = now();
      const memory: MemoryRecord | null = approved
        ? {
            id: createId("memory", ++counter),
            title: "Approved local file request",
            preview: action.target,
            source: "approved-action",
            conversationId: conversation.id,
            createdAt: now(),
            tags: ["approved", "local-file"],
          }
        : null;
      if (memory) memories = [memory, ...memories];
      return clone({
        conversation,
        summary: summaryFor(conversation),
        task,
        pendingActions: [],
        memory,
        memories: memory ? [memory] : [],
      });
    },

    async saveSettings(nextSettings: AppSettings): Promise<AppSettings> {
      await pause();
      settings = clone(nextSettings);
      return clone(settings);
    },

    async memoryStatus() {
      return { settings: clone(memorySettings), activeSources: crowQuantMemories.length + (memorySettings.indexConversations ? [...conversations.values()].reduce((n,c) => n+c.messages.length,0) : 0), chunks: 0, pending: 0, warnings: ["Development adapter: native persistence and indexing are tested in Rust."] };
    },
    async configureMemory(next: MemorySettings) { memorySettings = clone(next); return clone(next); },
    async searchMemory(query: MemoryQuery) {
      const candidates: NativeMemoryHit[] = [];
      if (memorySettings.indexConversations) {
        for (const conversation of conversations.values()) for (const message of conversation.messages) {
          candidates.push({chunkId:message.id,sourceId:message.id,sourceKind:"conversation_message",originId:message.id,title:conversation.title,authorship:message.role,text:message.content,createdAtMs:Date.parse(message.createdAt),startByte:0,endByte:new TextEncoder().encode(message.content).length,score:similarityScore(query.query,message.content),channels:[{channel:"development_keyword",rank:1,score:null}]});
        }
      }
      for (const record of crowQuantMemories) candidates.push({chunkId:record.id,sourceId:record.id,sourceKind:"user_note",originId:record.id,title:"Your note",authorship:"user",text:record.text,createdAtMs:Date.parse(record.createdAt),startByte:0,endByte:new TextEncoder().encode(record.text).length,score:similarityScore(query.query,record.text),channels:[{channel:"development_keyword",rank:1,score:null}]});
      return {hits:candidates.filter(h => !withdrawnSources.has(h.sourceId) && (!query.sourceKind || h.sourceKind===query.sourceKind) && h.score>0).sort((a,b)=>b.score-a.score).slice(0,query.limit),mode:query.mode,warnings:["Development adapter results; installed search uses native SQLite and CrowQuant."]};
    },
    async withdrawMemory(id: string) { withdrawnSources.add(id); },
    async syncMemory() { return {indexed:0,skipped:0,pending:0,warnings:[]}; },
    async rebuildMemory() { return {indexed:0,skipped:0,pending:0,warnings:[]}; },
    async exportMemory() { throw new Error("File export requires the installed native application."); },
    async admitFileMemory() { throw new Error("Development adapter has no retained real file result. Use the installed application."); },
    async syncSemanticMemory() { return {indexed:0,pending:0,warnings:["Development adapter does not run a real embedding model. Native protocol tests exercise the installed service."]}; },

    async listCrowQuantMemories(): Promise<CrowQuantMemory[]> {
      await pause();
      return clone(crowQuantMemories);
    },

    async rememberCrowQuant(text: string): Promise<CrowQuantMemory> {
      await pause();
      const normalized = text.trim();
      if (!normalized) throw new Error("Enter something for CrowQuant to remember.");
      const originalBytes = 256 * Float32Array.BYTES_PER_ELEMENT;
      const compressedBytes = 161;
      const memory: CrowQuantMemory = {
        id: createId("crowquant", ++counter),
        text: normalized,
        createdAt: now(),
        originalBytes,
        compressedBytes,
        compressionRatio: originalBytes / compressedBytes,
        algorithm: "CrowQuant WHT · 4-bit",
      };
      crowQuantMemories = [memory, ...crowQuantMemories];
      return clone(memory);
    },

    async recallCrowQuant(query: string, limit: number): Promise<CrowQuantSearchHit[]> {
      await pause();
      const normalized = query.trim();
      if (!normalized) throw new Error("Enter a query to recall CrowQuant memory.");
      const boundedLimit = Math.max(1, Math.min(20, Math.trunc(limit)));
      const hits = crowQuantMemories
        .map((memory) => ({ memory, score: similarityScore(normalized, memory.text) }))
        .sort((left, right) => right.score - left.score || right.memory.createdAt.localeCompare(left.memory.createdAt))
        .slice(0, boundedLimit);
      return clone(hits);
    },
  };
}
