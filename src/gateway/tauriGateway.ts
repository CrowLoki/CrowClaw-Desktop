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

export function createTauriGateway(): CrowClawGateway {
  return {
    bootstrap: () => invoke<AppBootstrap>(TAURI_COMMANDS.bootstrap),
    discoverEndpoints: () => invoke<DiscoveredEndpoint[]>(TAURI_COMMANDS.discoverEndpoints),
    testConnection: (draft: ModelEndpointDraft) =>
      invoke<ConnectionTestResult>(TAURI_COMMANDS.testConnection, { request: draft }),
    connectModel: (draft: ModelEndpointDraft) =>
      invoke<ModelConnection>(TAURI_COMMANDS.connectModel, { request: draft }),
    createConversation: () =>
      invoke<{ conversation: Conversation; summary: ConversationSummary }>(
        TAURI_COMMANDS.createConversation,
      ),
    getConversation: (conversationId: string) =>
      invoke<Conversation>(TAURI_COMMANDS.getConversation, { request: { conversationId } }),
    selectFolder: () => invoke<SelectedFolder | null>(TAURI_COMMANDS.selectFolder),
    sendMessage: (conversationId: string, content: string, selectedFolder: SelectedFolder | null) =>
      invoke<ChatTurnResult>(TAURI_COMMANDS.sendMessage, {
        request: { conversationId, content, selectedFolder },
      }),
    cancelTask: (taskId: string) =>
      invoke<TaskCancellationResult>(TAURI_COMMANDS.cancelTask, { request: { taskId } }),
    decideAction: (actionId: string, decision: ActionDecision) =>
      invoke<ActionDecisionResult>(TAURI_COMMANDS.decideAction, {
        request: { actionId, decision },
      }),
    saveSettings: (settings: AppSettings) =>
      invoke<AppSettings>(TAURI_COMMANDS.saveSettings, { request: settings }),
  };
}

export function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}
