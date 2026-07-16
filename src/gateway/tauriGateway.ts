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
  CrowClawGateway,
  DiscoveredEndpoint,
  ModelConnection,
  ModelEndpointDraft,
  SelectedFolder,
  TaskCancellationResult,
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
  };
}

export function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}
