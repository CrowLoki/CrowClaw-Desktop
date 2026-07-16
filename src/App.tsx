import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import { AppShell, type AppView } from "./components/AppShell";
import { ApprovalDialog } from "./components/ApprovalDialog";
import { ChatWorkspace } from "./components/ChatWorkspace";
import { ConnectionsView } from "./components/ConnectionsView";
import { ConversationSidebar } from "./components/ConversationSidebar";
import { ErrorScreen } from "./components/ErrorScreen";
import { LoadingScreen } from "./components/LoadingScreen";
import { MemoryView } from "./components/MemoryView";
import { Onboarding } from "./components/Onboarding";
import { SettingsView } from "./components/SettingsView";
import { TaskCenter } from "./components/TaskCenter";
import type {
  ActionDecision,
  AgentTask,
  AppBootstrap,
  AppSettings,
  ConnectionTestResult,
  Conversation,
  ConversationSummary,
  CrowClawGateway,
  DiscoveredEndpoint,
  ModelEndpointDraft,
  SelectedFolder,
} from "./gateway/contracts";
import { createCrowClawGateway } from "./gateway/gateway";
import { isTauriRuntime } from "./gateway/tauriGateway";

type AppProps = {
  gateway?: CrowClawGateway;
};

const defaultGateway = createCrowClawGateway();

function messageFrom(cause: unknown, fallback: string): string {
  return cause instanceof Error ? cause.message : fallback;
}

function upsertSummary(
  summaries: ConversationSummary[],
  summary: ConversationSummary,
): ConversationSummary[] {
  return [summary, ...summaries.filter(({ id }) => id !== summary.id)].sort((a, b) =>
    b.updatedAt.localeCompare(a.updatedAt),
  );
}

function upsertTask(tasks: AgentTask[], task: AgentTask): AgentTask[] {
  return [task, ...tasks.filter(({ id }) => id !== task.id)];
}

export function App({ gateway = defaultGateway }: AppProps) {
  const [bootstrap, setBootstrap] = useState<AppBootstrap | null>(null);
  const [conversation, setConversation] = useState<Conversation | null>(null);
  const [view, setView] = useState<AppView>("chat");
  const [loading, setLoading] = useState(true);
  const [conversationLoading, setConversationLoading] = useState(false);
  const [fatalError, setFatalError] = useState<string | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [sending, setSending] = useState(false);
  const [cancellingTaskId, setCancellingTaskId] = useState<string | null>(null);
  const [deciding, setDeciding] = useState<ActionDecision | null>(null);
  const [discovered, setDiscovered] = useState<DiscoveredEndpoint[]>([]);

  const loadConversation = useCallback(
    async (conversationId: string) => {
      setConversationLoading(true);
      setOperationError(null);
      try {
        setConversation(await gateway.getConversation(conversationId));
      } catch (cause) {
        setOperationError(messageFrom(cause, "The conversation could not be opened."));
      } finally {
        setConversationLoading(false);
      }
    },
    [gateway],
  );

  const loadApp = useCallback(async () => {
    setLoading(true);
    setFatalError(null);
    try {
      const next = await gateway.bootstrap();
      setBootstrap(next);
      if (!next.firstRun && next.selectedConversationId) {
        setConversation(await gateway.getConversation(next.selectedConversationId));
      } else {
        setConversation(null);
      }
    } catch (cause) {
      setFatalError(messageFrom(cause, "The native CrowClaw runtime did not respond."));
    } finally {
      setLoading(false);
    }
  }, [gateway]);

  useEffect(() => {
    void loadApp();
  }, [loadApp]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;
    let active = true;
    void listen<AgentTask>("crowclaw://task-updated", ({ payload }) => {
      setBootstrap((current) => current ? { ...current, tasks: upsertTask(current.tasks, payload) } : current);
    }).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const discoverEndpoints = useCallback(async () => {
    const endpoints = await gateway.discoverEndpoints();
    setDiscovered(endpoints);
    return endpoints;
  }, [gateway]);

  useEffect(() => {
    if (bootstrap && !bootstrap.firstRun && view === "connections" && discovered.length === 0) {
      void discoverEndpoints().catch((cause) =>
        setOperationError(messageFrom(cause, "Local endpoints could not be detected.")),
      );
    }
  }, [bootstrap, discoverEndpoints, discovered.length, view]);

  async function connectModel(draft: ModelEndpointDraft) {
    const connection = await gateway.connectModel(draft);
    setBootstrap((current) => current ? { ...current, firstRun: false, connection } : current);
    if (!bootstrap || bootstrap.firstRun) await loadApp();
  }

  async function createConversation() {
    setCreating(true);
    setOperationError(null);
    try {
      const created = await gateway.createConversation();
      setConversation(created.conversation);
      setBootstrap((current) =>
        current
          ? {
              ...current,
              conversations: upsertSummary(current.conversations, created.summary),
              selectedConversationId: created.conversation.id,
            }
          : current,
      );
      setView("chat");
    } catch (cause) {
      setOperationError(messageFrom(cause, "A new conversation could not be created."));
    } finally {
      setCreating(false);
    }
  }

  async function selectConversation(conversationId: string) {
    setBootstrap((current) => current ? { ...current, selectedConversationId: conversationId } : current);
    setView("chat");
    await loadConversation(conversationId);
  }

  async function sendMessage(content: string, selectedFolder: SelectedFolder | null) {
    if (!conversation) return;
    setSending(true);
    setOperationError(null);
    try {
      const result = await gateway.sendMessage(conversation.id, content, selectedFolder);
      setConversation(result.conversation);
      setBootstrap((current) => {
        if (!current) return current;
        return {
          ...current,
          conversations: upsertSummary(current.conversations, result.summary),
          tasks: upsertTask(current.tasks, result.task),
          pendingActions: [
            ...result.pendingActions,
            ...current.pendingActions.filter(({ taskId }) => taskId !== result.task.id),
          ],
        };
      });
    } catch (cause) {
      setOperationError(messageFrom(cause, "CrowClaw could not send that message."));
    } finally {
      setSending(false);
    }
  }

  async function cancelTask(taskId: string) {
    setCancellingTaskId(taskId);
    setOperationError(null);
    try {
      const result = await gateway.cancelTask(taskId);
      if (result.conversation?.id === conversation?.id) setConversation(result.conversation);
      setBootstrap((current) => {
        if (!current) return current;
        return {
          ...current,
          tasks: upsertTask(current.tasks, result.task),
          conversations: result.summary
            ? upsertSummary(current.conversations, result.summary)
            : current.conversations,
          pendingActions: current.pendingActions.filter(({ taskId: owner }) => owner !== taskId),
        };
      });
    } catch (cause) {
      setOperationError(messageFrom(cause, "The task could not be cancelled."));
    } finally {
      setCancellingTaskId(null);
    }
  }

  async function decideAction(decision: ActionDecision) {
    const action = bootstrap?.pendingActions[0];
    if (!action) return;
    setDeciding(decision);
    setOperationError(null);
    try {
      const result = await gateway.decideAction(action.id, decision);
      if (result.conversation.id === conversation?.id) setConversation(result.conversation);
      setBootstrap((current) => {
        if (!current) return current;
        const returnedMemories = result.memories.length > 0
          ? result.memories
          : result.memory
            ? [result.memory]
            : [];
        const returnedMemoryIds = new Set(returnedMemories.map(({ id }) => id));
        return {
          ...current,
          conversations: upsertSummary(current.conversations, result.summary),
          tasks: upsertTask(current.tasks, result.task),
          pendingActions: [
            ...result.pendingActions,
            ...current.pendingActions.filter(
              ({ id, taskId }) => id !== action.id && taskId !== result.task.id,
            ),
          ],
          memories: returnedMemories.length > 0
            ? [...returnedMemories, ...current.memories.filter(({ id }) => !returnedMemoryIds.has(id))]
            : current.memories,
        };
      });
    } catch (cause) {
      setOperationError(messageFrom(cause, "The approval decision could not be recorded."));
    } finally {
      setDeciding(null);
    }
  }

  async function saveSettings(settings: AppSettings) {
    const saved = await gateway.saveSettings(settings);
    setBootstrap((current) => current ? { ...current, settings: saved } : current);
  }

  const activeConversationTask = useMemo(() => {
    if (!conversation || !bootstrap) return null;
    return bootstrap.tasks.find(
      ({ conversationId, status }) =>
        conversationId === conversation.id && ["queued", "running", "waiting-approval"].includes(status),
    ) ?? null;
  }, [bootstrap, conversation]);

  if (loading) return <LoadingScreen />;
  if (fatalError) return <ErrorScreen message={fatalError} onRetry={() => void loadApp()} />;
  if (!bootstrap) return <ErrorScreen message="CrowClaw returned no workspace state." onRetry={() => void loadApp()} />;
  if (bootstrap.firstRun || !bootstrap.connection) {
    return (
      <Onboarding
        discoverEndpoints={discoverEndpoints}
        testConnection={(draft) => gateway.testConnection(draft)}
        connect={connectModel}
      />
    );
  }

  const sidebar = view === "chat" ? (
    <ConversationSidebar
      conversations={bootstrap.conversations}
      selectedId={bootstrap.selectedConversationId}
      creating={creating}
      onCreate={() => void createConversation()}
      onSelect={(id) => void selectConversation(id)}
    />
  ) : undefined;

  return (
    <AppShell
      view={view}
      connection={bootstrap.connection}
      tasks={bootstrap.tasks}
      developmentPreview={!isTauriRuntime() && import.meta.env.DEV}
      sidebar={sidebar}
      onViewChange={setView}
    >
      {view === "chat" && (
        <ChatWorkspace
          conversation={conversation}
          connection={bootstrap.connection}
          activeTask={activeConversationTask}
          loading={conversationLoading}
          sending={sending}
          error={operationError}
          onSelectFolder={() => gateway.selectFolder()}
          onSend={sendMessage}
        />
      )}
      {view === "tasks" && (
        <TaskCenter tasks={bootstrap.tasks} cancellingTaskId={cancellingTaskId} onCancel={(id) => void cancelTask(id)} />
      )}
      {view === "memory" && (
        <MemoryView
          memories={bootstrap.memories}
          listCrowQuantMemories={gateway.listCrowQuantMemories}
          rememberCrowQuant={gateway.rememberCrowQuant}
          recallCrowQuant={gateway.recallCrowQuant}
        />
      )}
      {view === "connections" && (
        <ConnectionsView
          connection={bootstrap.connection}
          discovered={discovered}
          onTest={(draft): Promise<ConnectionTestResult> => gateway.testConnection(draft)}
          onConnect={connectModel}
        />
      )}
      {view === "settings" && <SettingsView settings={bootstrap.settings} onSave={saveSettings} />}
      {bootstrap.pendingActions[0] && (
        <ApprovalDialog action={bootstrap.pendingActions[0]} deciding={deciding} onDecision={(decision) => void decideAction(decision)} />
      )}
    </AppShell>
  );
}

