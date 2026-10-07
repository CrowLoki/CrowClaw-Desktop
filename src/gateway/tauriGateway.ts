import { invoke } from "@tauri-apps/api/core";
import type {
  ActionDecision,
  ActionDecisionResult,
  AppBootstrap,
  AppSettings,
  ChatTurnResult,
  ConnectionTestResult,
  Conversation,
  ConversationSummary,
  CrowQuantMemory,
  CrowQuantSearchHit,
  CrowClawGateway,
  DiscoveredEndpoint,
  ModelConnection,
  ModelEndpointDraft,
  SelectedFolder,
  TaskCancellationResult,
  NativeMemoryStatus, MemorySettings, MemoryQuery, MemorySearchResult, MemoryIndexReport, SemanticIndexReport,
} from "./contracts";

export const TAURI_COMMANDS = {
  bootstrap: "crowclaw_app_bootstrap",
  discoverEndpoints: "crowclaw_model_discover",
  testConnection: "crowclaw_model_test_connection",
  connectModel: "crowclaw_model_connect",
  createConversation: "crowclaw_conversation_create",
  getConversation: "crowclaw_conversation_get",
  selectFolder: "crowclaw_folder_select",
  sendMessage: "crowclaw_chat_send",
  cancelTask: "crowclaw_task_cancel",
  decideAction: "crowclaw_action_decide",
  saveSettings: "crowclaw_settings_save",
  listCrowQuantMemories: "crowclaw_crowquant_list",
  rememberCrowQuant: "crowclaw_crowquant_remember",
  recallCrowQuant: "crowclaw_crowquant_recall",
  memoryStatus: "crowclaw_memory_status",
  configureMemory: "crowclaw_memory_configure",
  searchMemory: "crowclaw_memory_search",
  withdrawMemory: "crowclaw_memory_withdraw",
  syncMemory: "crowclaw_memory_sync",
  rebuildMemory: "crowclaw_memory_rebuild",
  exportMemory: "crowclaw_memory_export",
  admitFileMemory: "crowclaw_memory_admit_file",
  syncSemanticMemory: "crowclaw_memory_semantic_sync",
} as const;

async function invokeNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (cause) {
    if (cause instanceof Error) throw cause;
    if (typeof cause === "string") throw new Error(cause);
    if (cause && typeof cause === "object" && "message" in cause) {
      throw new Error(String(cause.message));
    }
    throw new Error(`CrowClaw native command ${command} failed.`);
  }
}

export function createTauriGateway(): CrowClawGateway {
  return {
    memoryStatus: () => invokeNative<NativeMemoryStatus>(TAURI_COMMANDS.memoryStatus),
    configureMemory: (request: MemorySettings) => invokeNative<MemorySettings>(TAURI_COMMANDS.configureMemory, { request }),
    searchMemory: (request: MemoryQuery) => invokeNative<MemorySearchResult>(TAURI_COMMANDS.searchMemory, { request }),
    withdrawMemory: (sourceId: string) => invokeNative<void>(TAURI_COMMANDS.withdrawMemory, { request: { sourceId } }),
    syncMemory: () => invokeNative<MemoryIndexReport>(TAURI_COMMANDS.syncMemory),
    rebuildMemory: () => invokeNative<MemoryIndexReport>(TAURI_COMMANDS.rebuildMemory),
    exportMemory: () => invokeNative<{ saved: boolean; fileName?: string }>(TAURI_COMMANDS.exportMemory),
    admitFileMemory: (actionId: string) => invokeNative<unknown>(TAURI_COMMANDS.admitFileMemory, { request: { actionId } }),
    syncSemanticMemory: () => invokeNative<SemanticIndexReport>(TAURI_COMMANDS.syncSemanticMemory),
    bootstrap: () => invokeNative<AppBootstrap>(TAURI_COMMANDS.bootstrap),
    discoverEndpoints: () => invokeNative<DiscoveredEndpoint[]>(TAURI_COMMANDS.discoverEndpoints),
    testConnection: (draft: ModelEndpointDraft) =>
      invokeNative<ConnectionTestResult>(TAURI_COMMANDS.testConnection, { request: draft }),
    connectModel: (draft: ModelEndpointDraft) =>
      invokeNative<ModelConnection>(TAURI_COMMANDS.connectModel, { request: draft }),
    createConversation: () =>
      invokeNative<{ conversation: Conversation; summary: ConversationSummary }>(
        TAURI_COMMANDS.createConversation,
      ),
    getConversation: (conversationId: string) =>
      invokeNative<Conversation>(TAURI_COMMANDS.getConversation, { request: { conversationId } }),
    selectFolder: () => invokeNative<SelectedFolder | null>(TAURI_COMMANDS.selectFolder),
    sendMessage: (conversationId: string, content: string, selectedFolder: SelectedFolder | null) =>
      invokeNative<ChatTurnResult>(TAURI_COMMANDS.sendMessage, {
        request: { conversationId, content, selectedFolder },
      }),
    cancelTask: (taskId: string) =>
      invokeNative<TaskCancellationResult>(TAURI_COMMANDS.cancelTask, { request: { taskId } }),
    decideAction: (actionId: string, decision: ActionDecision) =>
      invokeNative<ActionDecisionResult>(TAURI_COMMANDS.decideAction, {
        request: { actionId, decision },
      }),
    saveSettings: (settings: AppSettings) =>
      invokeNative<AppSettings>(TAURI_COMMANDS.saveSettings, { request: settings }),
    listCrowQuantMemories: () =>
      invokeNative<CrowQuantMemory[]>(TAURI_COMMANDS.listCrowQuantMemories),
    rememberCrowQuant: (text: string) =>
      invokeNative<CrowQuantMemory>(TAURI_COMMANDS.rememberCrowQuant, { request: { text } }),
    recallCrowQuant: (query: string, limit: number) =>
      invokeNative<CrowQuantSearchHit[]>(TAURI_COMMANDS.recallCrowQuant, {
        request: { query, limit },
      }),
  };
}

export function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}
