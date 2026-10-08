import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import { AppShell, type AppView } from "./components/AppShell";
import { ApprovalDialog } from "./components/ApprovalDialog";
import { ChatWorkspace } from "./components/ChatWorkspace";
import { ConnectionsView } from "./components/ConnectionsView";
import { ConversationSidebar } from "./components/ConversationSidebar";
import { ErrorScreen } from "./components/ErrorScreen";
import { EvolutionView } from "./components/EvolutionView";
import { LoadingScreen } from "./components/LoadingScreen";
import { MemoryView } from "./components/MemoryView";
import { Onboarding } from "./components/Onboarding";
import { SettingsView } from "./components/SettingsView";
import { TaskCenter } from "./components/TaskCenter";
import { MembershipAccounts, MembershipUsage, MembershipWelcome } from "./components/MembershipAccounts";
import { useMembershipAccounts } from "./components/useMembershipAccounts";
import { useConversationComposer } from './components/useConversationComposer';
import { ComposerModelControls } from './components/ComposerModelControls';
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
  MemoryRecord,
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

export function mergeMemoryRecords(
  current: MemoryRecord[],
  returned: MemoryRecord[],
): MemoryRecord[] {
  const seen = new Set<string>();
  return [...returned, ...current].filter(({ id }) => {
    if (seen.has(id)) return false;
    seen.add(id);
    return true;
  });
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
  const [sendingIds, setSendingIds] = useState<Set<string>>(new Set());
  const sendingRef = useRef(new Set<string>());
  const [sendErrors, setSendErrors] = useState<Record<string,string|null>>({});
  const [conversationFolders, setConversationFolders] = useState<Record<string,SelectedFolder|null>>({});
  const loadGeneration = useRef(0);
  const composer = useConversationComposer(gateway,conversation?.id ?? null);
  const previousView = useRef(view);
  useEffect(() => {
    const returningToChat = previousView.current !== 'chat' && view === 'chat';
    previousView.current = view;
    if (returningToChat) void composer.sync().catch(() => undefined);
  }, [view, composer.sync]);
  const sending = conversation !== null && sendingIds.has(conversation.id);
  const [cancellingTaskId, setCancellingTaskId] = useState<string | null>(null);
  const [deciding, setDeciding] = useState<ActionDecision | null>(null);
  const [discovered, setDiscovered] = useState<DiscoveredEndpoint[]>([]);
  const membership = useMembershipAccounts(gateway,
    (connection) => {
      setBootstrap((current) => current ? { ...current, firstRun: false, connection } : current);
      if (bootstrap?.firstRun && bootstrap.selectedConversationId) void loadConversation(bootstrap.selectedConversationId);
    },
    (accountId) => setBootstrap((current) => current?.connection?.provider === "chatgpt" && current.connection.id === `membership:${accountId}`
      ? { ...current, connection: { ...current.connection, status: "disconnected", connectedAt: null, latencyMs: null } } : current),
  );

  const loadConversation = useCallback(
    async (conversationId: string) => {
      const generation=++loadGeneration.current;
      setConversationLoading(true);
      setOperationError(null);
      try {
        const loaded=await gateway.getConversation(conversationId);
        if(generation===loadGeneration.current)setConversation(loaded);
      } catch (cause) {
        if(generation===loadGeneration.current)setOperationError(messageFrom(cause, "The conversation could not be opened."));
      } finally {
        if(generation===loadGeneration.current)setConversationLoading(false);
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
    ++loadGeneration.current;
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
    const conversationId=conversation.id;
    if(sendingRef.current.has(conversationId))throw new Error('A message is already being sent in this conversation.');
    sendingRef.current.add(conversationId);
    const submittedDraft=composer.draft;
    setSendingIds(current=>new Set(current).add(conversationId));
    setSendErrors(current=>({...current,[conversationId]:null}));
    setOperationError(null);
    let responseReceived=false;
    try {
      const revision=await composer.flush();
      const result = await gateway.sendMessage(conversationId, content, selectedFolder,revision);
      responseReceived=true;
      setConversation(current=>current?.id===conversationId?result.conversation:current);
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
      await composer.submitted(conversationId,submittedDraft);
    } catch (cause) {
      const detail=messageFrom(cause, "CrowClaw could not send that message.");
      setSendErrors(current=>({...current,[conversationId]:responseReceived?`Response saved, but composer refresh failed: ${detail}`:detail}));
      throw cause;
    } finally {
      sendingRef.current.delete(conversationId);
      setSendingIds(current=>{const next=new Set(current);next.delete(conversationId);return next;});
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
            ? mergeMemoryRecords(current.memories, returnedMemories)
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
      <><Onboarding
        discoverEndpoints={discoverEndpoints}
        testConnection={(draft) => gateway.testConnection(draft)}
        connect={connectModel}
        membership={<MembershipAccounts membership={membership} connection={bootstrap.connection} />}
      />
      <MembershipWelcome membership={membership} /></>
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

  const chatConnection = composer.snapshot?.connection ?? {...bootstrap.connection,status:'disconnected' as const};

  return (
    <><AppShell
      view={view}
      connection={view==='chat'?chatConnection:bootstrap.connection}
      tasks={bootstrap.tasks}
      developmentPreview={!isTauriRuntime() && import.meta.env.DEV}
      sidebar={sidebar}
      onViewChange={setView}
    >
      {view === "chat" && (
        <ChatWorkspace
          conversation={conversation}
          connection={chatConnection}
          activeTask={activeConversationTask}
          loading={conversationLoading}
          sending={sending}
          error={operationError ?? (conversation ? sendErrors[conversation.id] : null) ?? composer.error ?? composer.snapshot?.warning ?? null}
          draft={composer.draft}
          onDraftChange={composer.setDraft}
          composerLoading={conversationLoading || composer.loading}
          selectedFolder={conversation ? conversationFolders[conversation.id] ?? null : null}
          onSelectedFolderChange={folder=>{if(conversation)setConversationFolders(current=>({...current,[conversation.id]:folder}));}}
          modelControls={<><ComposerModelControls key={conversation?.id ?? 'none'} value={composer.snapshot?.composer.selection ?? null} sources={composer.snapshot?.sources ?? []} busy={conversationLoading || composer.loading || sending} onChoose={composer.choose} onRefresh={composer.refreshSource} />
            {(composer.draft || composer.saving) && <span role="status">{composer.saving ? 'Saving draft…' : composer.snapshot?.composer.draft === composer.draft ? 'Draft saved on this device.' : 'Draft kept in this window; not saved yet.'}</span>}
            {(composer.error || operationError || (conversation && sendErrors[conversation.id])) && <button type="button" className="button button--secondary" disabled={composer.loading || sending} onClick={()=>void composer.refresh().then(()=>{setOperationError(null);if(conversation)setSendErrors(current=>({...current,[conversation.id]:null}));}).catch(()=>undefined)}>Refresh composer</button>}
          </>}
          membershipUsage={<MembershipUsage membership={membership} />}
          onSelectFolder={() => gateway.selectFolder()}
          onSend={sendMessage}
        />
      )}
      {view === "tasks" && (
        <TaskCenter tasks={bootstrap.tasks} cancellingTaskId={cancellingTaskId} onCancel={(id) => void cancelTask(id)} />
      )}
      {view === "memory" && (
        <MemoryView
          gateway={gateway}
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
      {view === "evolution" && <EvolutionView gateway={gateway} connection={bootstrap.connection} developmentPreview={!isTauriRuntime() && import.meta.env.DEV} />}
      {view === "settings" && <SettingsView settings={bootstrap.settings} onSave={saveSettings} membership={<MembershipAccounts membership={membership} connection={bootstrap.connection} />} />}
      {bootstrap.pendingActions[0] && (
        <ApprovalDialog action={bootstrap.pendingActions[0]} deciding={deciding} onDecision={(decision) => void decideAction(decision)} />
      )}
    </AppShell>
    <MembershipWelcome membership={membership} /></>
  );
}
