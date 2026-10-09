import type { ConversationComposerSnapshot, ConversationModelChoice, ComposerModelSource } from './composerContracts';
import type { AttachmentPreview, AttachmentSummary } from './attachmentContracts';
import type { FreeCatalog, OpenRouterConnectRequest } from './openRouterContracts';

export type ProviderKind = "lm-studio" | "ollama" | "llama-cpp" | "crowbot-ai" | "custom";

export type ModelEndpointDraft = {
  provider: ProviderKind;
  label: string;
  baseUrl: string;
  model: string;
  apiKey?: string;
};

export type ModelConnection = Omit<ModelEndpointDraft, "apiKey" | "provider"> & {
  provider: ProviderKind | "chatgpt" | "openrouter";
  id: string;
  status: "connected" | "disconnected" | "error";
  connectedAt: string | null;
  latencyMs: number | null;
};

export type MembershipAccount = {
  id: string;
  label: string;
  identity: {
    provider: string;
    issuer: string;
    subject: string;
    clientId: string;
    hostId: string;
    email: string | null;
  };
  hasCredentials: boolean;
  credentialVersion: number;
  catalog: {
    accountId: string;
    models: Array<{ slug: string; displayName: string; reasoningEfforts: string[] }>;
    fetchedAtMs: number;
  } | null;
  selection: { accountId: string; model: string; reasoningEffort: string | null } | null;
  createdAtMs: number;
  updatedAtMs: number;
};

export type MembershipSnapshot = { accounts: MembershipAccount[]; welcomeAcknowledged: boolean };
export type MembershipSignInRequest = { requestId: string; label: string; accountId: string | null };
export type MembershipModelRequest = { accountId: string; model: string; reasoningEffort: string | null };
export type MembershipSignOutResult = { account: MembershipAccount; remoteRevoked: boolean; detail: string };
export type CodexImageAuthStatus = {
  state: "not_connected" | "pending" | "connected" | "expired";
  verificationUrl: string | null;
  userCode: string | null;
  pollIntervalSeconds: number | null;
  message: string | null;
};

export type DiscoveredEndpoint = ModelEndpointDraft & {
  id: string;
  detected: boolean;
  availableModels: string[];
};

export type ConnectionTestResult = {
  ok: boolean;
  latencyMs: number | null;
  resolvedModel: string | null;
  detail: string;
};

export type MessageRole = "user" | "assistant" | "system";
export type MessageStatus = "sent" | "streaming" | "waiting-approval" | "failed";

export type ConversationMessage = {
  attachments?: AttachmentSummary[];
  id: string;
  role: MessageRole;
  content: string;
  createdAt: string;
  status: MessageStatus;
  taskId?: string;
  requestedModel?: string;
  reportedModel?: string;
  reasoningEffort?: string;
};

export type ConversationSummary = {
  id: string;
  title: string;
  preview: string;
  updatedAt: string;
  unread: boolean;
};

export type Conversation = {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  messages: ConversationMessage[];
};

export type SelectedFolder = {
  id: string;
  name: string;
  displayPath: string;
};

export type AgentTaskStatus =
  | "queued"
  | "running"
  | "waiting-approval"
  | "completed"
  | "cancelled"
  | "failed";

export type AgentTask = {
  id: string;
  conversationId: string;
  title: string;
  detail: string;
  status: AgentTaskStatus;
  progress: number | null;
  startedAt: string;
  updatedAt: string;
  cancellable: boolean;
};

export type ActionRisk = "low" | "medium" | "high";

export type PendingAction = {
  id: string;
  taskId: string;
  conversationId: string;
  kind: "read-files" | "write-file" | "run-command" | "open-application" | "memory" | "image-generation";
  title: string;
  summary: string;
  target: string;
  details: string[];
  risk: ActionRisk;
  requestedAt: string;
};

export type ActionDecision = "approved" | "denied";

export type MemoryRecord = {
  id: string;
  title: string;
  preview: string;
  source: "conversation" | "approved-action" | "user-note";
  conversationId: string | null;
  createdAt: string;
  tags: string[];
};

export type CrowQuantMemory = {
  id: string;
  text: string;
  createdAt: string;
  originalBytes: number;
  compressedBytes: number;
  compressionRatio: number;
  algorithm: string;
};

export type CrowQuantSearchHit = {
  memory: CrowQuantMemory;
  score: number;
};

export type PermissionMode = "ask" | "allow-session" | "deny";

export type EmbeddingProfile = { provider: "openai" | "ollama"; baseUrl: string; model: string; dimensions: number };
export type MemorySettings = { indexConversations: boolean | null; indexActions: boolean; embedding?: EmbeddingProfile | null };
export type MemorySearchMode = "hybrid" | "full_text" | "lexical" | "semantic";
export type MemoryQuery = { query: string; limit: number; sourceKind: string | null; mode: MemorySearchMode };
export type NativeMemoryHit = {
  chunkId: string; sourceId: string; sourceKind: string; originId: string; title: string;
  authorship: string; text: string; createdAtMs: number; startByte: number; endByte: number;
  score: number; channels: Array<{ channel: string; rank: number; score: number | null }>;
};
export type MemorySearchResult = { hits: NativeMemoryHit[]; mode: MemorySearchMode; warnings: string[] };
export type SemanticStatus = { state: "disabled" | "pending" | "indexed" | "degraded"; profileId: string | null; vectors: number; pending: number; detail: string | null };
export type NativeMemoryStatus = { settings: MemorySettings; activeSources: number; chunks: number; pending: number; warnings: string[]; semantic?: SemanticStatus };
export type MemoryIndexReport = { indexed: number; skipped: number; pending: number; warnings: string[] };
export type SemanticIndexReport = { indexed: number; pending: number; warnings: string[] };

export type AppSettings = {
  personalities?:Array<{id:string;name:string;instruction:string}>;
  selectedPersonality?:string|null;
  permissions: {
    readFiles: PermissionMode;
    writeFiles: PermissionMode;
    runCommands: PermissionMode;
  };
  launchAtLogin: boolean;
  keepRunningOnClose: boolean;
  retainConversations: boolean;
  theme: "system" | "dark";
};

export type AppBootstrap = {
  firstRun: boolean;
  connection: ModelConnection | null;
  conversations: ConversationSummary[];
  selectedConversationId: string | null;
  tasks: AgentTask[];
  pendingActions: PendingAction[];
  memories: MemoryRecord[];
  settings: AppSettings;
};

export type ChatTurnResult = {
  conversation: Conversation;
  summary: ConversationSummary;
  task: AgentTask;
  pendingActions: PendingAction[];
};

export type ActionDecisionResult = {
  conversation: Conversation;
  summary: ConversationSummary;
  task: AgentTask;
  pendingActions: PendingAction[];
  memory: MemoryRecord | null;
  memories: MemoryRecord[];
};

export type TaskCancellationResult = {
  task: AgentTask;
  conversation: Conversation | null;
  summary: ConversationSummary | null;
};

export type EvolutionRating = "useful" | "needs_improvement" | "uncertain";
export type EvolutionFeedback = { rating: EvolutionRating; note: string; updatedAtMs: number };
export type EvolutionObservation = { taskId: string; title: string; outcome: "succeeded" | "failed" | "cancelled"; guidelineRevision: number | null; updatedAtMs: number; feedback: EvolutionFeedback | null };
export type EvolutionRevision = { revision: number; title: string; instructions: string; reason: string; createdAtMs: number };
export type EvolutionProposal = { id: string; baseRevision: number; title: string; rationale: string; instructions: string; sourceTaskIds: string[]; model: string | null; reportedModel: string | null; reflectionContext: Record<string, unknown> | null; status: "draft" | "applied" | "rejected"; createdAtMs: number; decidedAtMs: number | null; appliedRevision: number | null };
export type EvolutionEvaluation = { id: string; proposalId: string; baselineRevision: number; model: string; baselineModel: string | null; candidateModel: string | null; candidateInstructions: string; prompt: string; baselineResponse: string; candidateResponse: string; preference: "baseline" | "candidate" | "tie" | "neither" | null; createdAtMs: number };
export type EvolutionSnapshot = { active: EvolutionRevision; observations: EvolutionObservation[]; proposals: EvolutionProposal[]; revisions: EvolutionRevision[]; evaluations: EvolutionEvaluation[] };
export type EvolutionDraftRequest = { title: string; rationale: string; instructions: string; sourceTaskIds: string[]; baseRevision: number };

export interface CrowClawGateway {
  openRouterCatalog(profileId?: string): Promise<FreeCatalog>;
  connectOpenRouter(request: OpenRouterConnectRequest): Promise<ModelConnection>;
  disconnectOpenRouter(profileId: string): Promise<void>;
  selectAttachments(conversationId: string, revision: number): Promise<ConversationComposerSnapshot>;
  removeAttachment(conversationId: string, revision: number, attachmentId: string): Promise<ConversationComposerSnapshot>;
  previewAttachment(conversationId: string, attachmentId: string): Promise<AttachmentPreview>;
  getComposer(conversationId: string): Promise<ConversationComposerSnapshot>;
  saveComposerDraft(conversationId: string, revision: number, draft: string): Promise<ConversationComposerSnapshot>;
  chooseComposerModel(conversationId: string, revision: number, selection: ConversationModelChoice): Promise<ConversationComposerSnapshot>;
  refreshComposerModels(sourceId: string): Promise<ComposerModelSource>;
  setHiddenModels(hiddenModelKeys: string[]): Promise<string[]>;
  membershipSnapshot(): Promise<MembershipSnapshot>;
  signInMembership(request: MembershipSignInRequest): Promise<MembershipAccount>;
  cancelMembershipSignIn(requestId: string): Promise<boolean>;
  signOutMembership(accountId: string): Promise<MembershipSignOutResult>;
  refreshMembershipModels(accountId: string): Promise<MembershipAccount>;
  useMembershipModel(request: MembershipModelRequest): Promise<ModelConnection>;
  acknowledgeMembershipWelcome(): Promise<void>;
  manageMembershipUsage(): Promise<void>;
  beginCodexImageAuthorization(accountId: string): Promise<CodexImageAuthStatus>;
  pollCodexImageAuthorization(accountId: string): Promise<CodexImageAuthStatus>;
  codexImageAuthorizationStatus(accountId: string): Promise<CodexImageAuthStatus>;
  cancelCodexImageAuthorization(accountId: string): Promise<boolean>;
  evolutionSnapshot(): Promise<EvolutionSnapshot>;
  saveEvolutionFeedback(request: { taskId: string; rating: EvolutionRating; note: string }): Promise<void>;
  draftEvolution(request: EvolutionDraftRequest): Promise<EvolutionProposal>;
  reflectEvolution(request: { requestId: string; taskId: string; goal: string }): Promise<EvolutionProposal>;
  evaluateEvolution(request: { requestId: string; proposalId: string; instructions: string; prompt: string }): Promise<EvolutionEvaluation>;
  rateEvolutionEvaluation(request: { id: string; preference: "baseline" | "candidate" | "tie" | "neither" }): Promise<void>;
  decideEvolution(request: { id: string; decision: "apply" | "reject"; instructions: string; expectedRevision: number }): Promise<EvolutionProposal>;
  restoreEvolution(request: { revision: number; expectedRevision: number }): Promise<EvolutionRevision>;
  cancelEvolution(requestId: string): Promise<void>;
  bootstrap(): Promise<AppBootstrap>;
  discoverEndpoints(): Promise<DiscoveredEndpoint[]>;
  testConnection(draft: ModelEndpointDraft): Promise<ConnectionTestResult>;
  connectModel(draft: ModelEndpointDraft): Promise<ModelConnection>;
  createConversation(): Promise<{ conversation: Conversation; summary: ConversationSummary }>;
  getConversation(conversationId: string): Promise<Conversation>;
  selectFolder(): Promise<SelectedFolder | null>;
  sendMessage(
    conversationId: string,
    content: string,
    selectedFolder: SelectedFolder | null,
    composerRevision?: number,
  ): Promise<ChatTurnResult>;
  cancelTask(taskId: string): Promise<TaskCancellationResult>;
  decideAction(actionId: string, decision: ActionDecision): Promise<ActionDecisionResult>;
  saveSettings(settings: AppSettings): Promise<AppSettings>;
  listCrowQuantMemories(): Promise<CrowQuantMemory[]>;
  rememberCrowQuant(text: string): Promise<CrowQuantMemory>;
  recallCrowQuant(query: string, limit: number): Promise<CrowQuantSearchHit[]>;
  memoryStatus(): Promise<NativeMemoryStatus>;
  configureMemory(settings: MemorySettings): Promise<MemorySettings>;
  searchMemory(query: MemoryQuery): Promise<MemorySearchResult>;
  withdrawMemory(sourceId: string): Promise<void>;
  syncMemory(): Promise<MemoryIndexReport>;
  rebuildMemory(): Promise<MemoryIndexReport>;
  exportMemory(): Promise<{ saved: boolean; fileName?: string }>;
  admitFileMemory(actionId: string): Promise<unknown>;
  syncSemanticMemory(): Promise<SemanticIndexReport>;
}
