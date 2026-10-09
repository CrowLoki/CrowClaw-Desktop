import { invoke } from "@tauri-apps/api/core";
import type { FreeCatalog } from './openRouterContracts';
import type { AttachmentPreview } from './attachmentContracts';
import type { ConversationComposerSnapshot, ComposerModelSource } from './composerContracts';
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
  EvolutionSnapshot, EvolutionProposal, EvolutionEvaluation, EvolutionRevision,
  MembershipAccount, MembershipSnapshot, MembershipSignOutResult,
  CodexImageAuthStatus,
} from "./contracts";

export const TAURI_COMMANDS = {
  openRouterCatalog: "crowclaw_openrouter_catalog",
  connectOpenRouter: "crowclaw_openrouter_connect",
  disconnectOpenRouter: "crowclaw_openrouter_disconnect",
  selectAttachments: "crowclaw_attachments_select",
  removeAttachment: "crowclaw_attachment_remove",
  previewAttachment: "crowclaw_attachment_preview",
  getComposer: "crowclaw_composer_get",
  saveComposerDraft: "crowclaw_composer_save_draft",
  chooseComposerModel: "crowclaw_composer_choose",
  refreshComposerModels: "crowclaw_composer_refresh_models",
  membershipSnapshot: "crowclaw_membership_snapshot",
  signInMembership: "crowclaw_membership_sign_in",
  cancelMembershipSignIn: "crowclaw_membership_cancel_sign_in",
  signOutMembership: "crowclaw_membership_sign_out",
  refreshMembershipModels: "crowclaw_membership_refresh_models",
  beginCodexImageAuthorization: "crowclaw_codex_image_auth_begin",
  pollCodexImageAuthorization: "crowclaw_codex_image_auth_poll",
  codexImageAuthorizationStatus: "crowclaw_codex_image_auth_status",
  cancelCodexImageAuthorization: "crowclaw_codex_image_auth_cancel",
  useMembershipModel: "crowclaw_membership_use_model",
  acknowledgeMembershipWelcome: "crowclaw_membership_acknowledge_welcome",
  manageMembershipUsage: "crowclaw_membership_manage_usage",
  evolutionSnapshot: "crowclaw_evolution_snapshot",
  saveEvolutionFeedback: "crowclaw_evolution_feedback",
  draftEvolution: "crowclaw_evolution_draft",
  reflectEvolution: "crowclaw_evolution_reflect",
  evaluateEvolution: "crowclaw_evolution_evaluate",
  rateEvolutionEvaluation: "crowclaw_evolution_rate",
  decideEvolution: "crowclaw_evolution_decide",
  restoreEvolution: "crowclaw_evolution_restore",
  cancelEvolution: "crowclaw_evolution_cancel",
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
    openRouterCatalog: (profileId) => invokeNative<FreeCatalog>(TAURI_COMMANDS.openRouterCatalog, { request: { profileId: profileId ?? null } }),
    connectOpenRouter: (request) => invokeNative<ModelConnection>(TAURI_COMMANDS.connectOpenRouter, { request }),
    disconnectOpenRouter: (profileId) => invokeNative<void>(TAURI_COMMANDS.disconnectOpenRouter, { request: { profileId } }),
    selectAttachments: (conversationId, revision) => invokeNative<ConversationComposerSnapshot>(TAURI_COMMANDS.selectAttachments, { request: { conversationId, revision } }),
    removeAttachment: (conversationId, revision, attachmentId) => invokeNative<ConversationComposerSnapshot>(TAURI_COMMANDS.removeAttachment, { request: { conversationId, revision, attachmentId } }),
    previewAttachment: (conversationId, attachmentId) => invokeNative<AttachmentPreview>(TAURI_COMMANDS.previewAttachment, { request: { conversationId, attachmentId } }),
    getComposer: (conversationId) => invokeNative<ConversationComposerSnapshot>(TAURI_COMMANDS.getComposer,{request:{conversationId}}),
    saveComposerDraft: (conversationId,revision,draft) => invokeNative<ConversationComposerSnapshot>(TAURI_COMMANDS.saveComposerDraft,{request:{conversationId,revision,draft}}),
    chooseComposerModel: (conversationId,revision,selection) => invokeNative<ConversationComposerSnapshot>(TAURI_COMMANDS.chooseComposerModel,{request:{conversationId,revision,selection}}),
    refreshComposerModels: (sourceId) => invokeNative<ComposerModelSource>(TAURI_COMMANDS.refreshComposerModels,{request:{sourceId}}),
    membershipSnapshot: () => invokeNative<MembershipSnapshot>(TAURI_COMMANDS.membershipSnapshot),
    signInMembership: (request) => invokeNative<MembershipAccount>(TAURI_COMMANDS.signInMembership, { request }),
    cancelMembershipSignIn: (requestId) => invokeNative<boolean>(TAURI_COMMANDS.cancelMembershipSignIn, { requestId }),
    signOutMembership: (accountId) => invokeNative<MembershipSignOutResult>(TAURI_COMMANDS.signOutMembership, { accountId }),
    refreshMembershipModels: (accountId) => invokeNative<MembershipAccount>(TAURI_COMMANDS.refreshMembershipModels, { accountId }),
    beginCodexImageAuthorization: (accountId) => invokeNative<CodexImageAuthStatus>(TAURI_COMMANDS.beginCodexImageAuthorization,{accountId}),
    pollCodexImageAuthorization: (accountId) => invokeNative<CodexImageAuthStatus>(TAURI_COMMANDS.pollCodexImageAuthorization,{accountId}),
    codexImageAuthorizationStatus: (accountId) => invokeNative<CodexImageAuthStatus>(TAURI_COMMANDS.codexImageAuthorizationStatus,{accountId}),
    cancelCodexImageAuthorization: (accountId) => invokeNative<boolean>(TAURI_COMMANDS.cancelCodexImageAuthorization,{accountId}),
    useMembershipModel: (request) => invokeNative<ModelConnection>(TAURI_COMMANDS.useMembershipModel, { request }),
    acknowledgeMembershipWelcome: () => invokeNative<void>(TAURI_COMMANDS.acknowledgeMembershipWelcome),
    manageMembershipUsage: () => invokeNative<void>(TAURI_COMMANDS.manageMembershipUsage),
    evolutionSnapshot: () => invokeNative<EvolutionSnapshot>(TAURI_COMMANDS.evolutionSnapshot),
    saveEvolutionFeedback: (request) => invokeNative<void>(TAURI_COMMANDS.saveEvolutionFeedback, { request }),
    draftEvolution: (request) => invokeNative<EvolutionProposal>(TAURI_COMMANDS.draftEvolution, { request }),
    reflectEvolution: (request) => invokeNative<EvolutionProposal>(TAURI_COMMANDS.reflectEvolution, { request }),
    evaluateEvolution: (request) => invokeNative<EvolutionEvaluation>(TAURI_COMMANDS.evaluateEvolution, { request }),
    rateEvolutionEvaluation: (request) => invokeNative<void>(TAURI_COMMANDS.rateEvolutionEvaluation, { request }),
    decideEvolution: (request) => invokeNative<EvolutionProposal>(TAURI_COMMANDS.decideEvolution, { request }),
    restoreEvolution: (request) => invokeNative<EvolutionRevision>(TAURI_COMMANDS.restoreEvolution, { request }),
    cancelEvolution: (requestId) => invokeNative<void>(TAURI_COMMANDS.cancelEvolution, { request: { requestId } }),
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
    sendMessage: (conversationId: string, content: string, selectedFolder: SelectedFolder | null, composerRevision?: number) =>
      invokeNative<ChatTurnResult>(TAURI_COMMANDS.sendMessage, {
        request: { conversationId, content, selectedFolder, ...(composerRevision === undefined ? {} : {composerRevision}) },
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
