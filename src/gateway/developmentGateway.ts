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
  DiscoveredEndpoint,
  MemoryRecord,
  ModelConnection,
  ModelEndpointDraft,
  PendingAction,
  SelectedFolder,
  TaskCancellationResult,
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

function now(): string {
  return new Date().toISOString();
}

function createId(prefix: string, counter: number): string {
  return `${prefix}-${counter.toString().padStart(4, "0")}`;
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
  }

  return {
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
      connection = {
        id: createId("connection", ++counter),
        provider: draft.provider,
        label: draft.label.trim() || "Local endpoint",
        baseUrl: draft.baseUrl.trim(),
        model: draft.model.trim(),
        status: "connected",
        connectedAt: now(),
        latencyMs: tested.latencyMs,
      };
      firstRun = false;
      return clone(connection);
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
    ): Promise<ChatTurnResult> {
      await pause();
      if (!connection || connection.status !== "connected") {
        throw new Error("Connect a local model before sending a message.");
      }
      const conversation = requireConversation(conversationId);
      const timestamp = now();
      const userMessage: ConversationMessage = {
        id: createId("message", ++counter),
        role: "user",
        content: content.trim(),
        createdAt: timestamp,
        status: "sent",
      };
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
          content: `I’m connected through ${connection.label} using ${connection.model}. This development preview confirms the desktop conversation flow; native model streaming is supplied by the Tauri runtime.`,
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
        pendingAction,
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
      return clone({ conversation, summary: summaryFor(conversation), task, memory });
    },

    async saveSettings(nextSettings: AppSettings): Promise<AppSettings> {
      await pause();
      settings = clone(nextSettings);
      return clone(settings);
    },
  };
}
