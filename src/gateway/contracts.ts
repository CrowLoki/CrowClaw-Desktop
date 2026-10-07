export type ProviderKind = "lm-studio" | "ollama" | "llama-cpp" | "custom";

export type ModelEndpointDraft = {
  provider: ProviderKind;
  label: string;
  baseUrl: string;
  model: string;
  apiKey?: string;
};

export type ModelConnection = Omit<ModelEndpointDraft, "apiKey"> & {
  id: string;
  status: "connected" | "disconnected" | "error";
  connectedAt: string | null;
  latencyMs: number | null;
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
  id: string;
  role: MessageRole;
  content: string;
  createdAt: string;
  status: MessageStatus;
  taskId?: string;
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
  kind: "read-files" | "write-file" | "run-command" | "open-application" | "memory";
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

export interface CrowClawGateway {
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
