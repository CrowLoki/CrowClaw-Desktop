import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createTauriGateway, TAURI_COMMANDS } from "./tauriGateway";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

describe("Tauri command contract", () => {
  beforeEach(() => invokeMock.mockReset());
  it('routes an explicit Edge choice separately from account registration and OAuth material',async()=>{
    invokeMock.mockResolvedValue(undefined);
    await createTauriGateway().signInMembership({requestId:'edge-request',label:'Edge account',accountId:null,browser:'edge'});
    expect(invokeMock).toHaveBeenCalledWith('crowclaw_membership_sign_in',{request:{requestId:'edge-request',label:'Edge account',accountId:null},browser:'edge'});
  });

  it('uses dedicated OpenRouter envelopes and preserves the backend free-only catalog without guessed model choices', async () => {
    const gateway = createTauriGateway();
    const catalog = { models: [{ id: 'returned-free-id', name: 'Returned name', contextLength: 4096, inputModalities: ['text'], supportedParameters: ['tools'], reasoningEfforts: [] }], fetchedAtMs: 123 };
    invokeMock.mockResolvedValue(catalog);
    await expect(gateway.openRouterCatalog()).resolves.toEqual(catalog);
    await expect(gateway.openRouterCatalog('openrouter:one')).resolves.toEqual(catalog);
    const request = { label: 'Free', apiKey: 'test-key', model: 'returned-free-id' };
    await gateway.connectOpenRouter(request);
    await gateway.disconnectOpenRouter('openrouter:one');
    expect(invokeMock.mock.calls).toEqual([
      ['crowclaw_openrouter_catalog', { request: { profileId: null } }],
      ['crowclaw_openrouter_catalog', { request: { profileId: 'openrouter:one' } }],
      ['crowclaw_openrouter_connect', { request }],
      ['crowclaw_openrouter_disconnect', { request: { profileId: 'openrouter:one' } }],
    ]);
  });

  it('uses attachment request envelopes with only native identifiers and revisions', async () => {
    const gateway = createTauriGateway();
    const snapshot = { composer: { conversationId: 'chat-a', revision: 8, draft: '', selection: null, attachments: [] }, connection: null, sources: [], warning: null };
    invokeMock.mockResolvedValue(snapshot);
    await expect(gateway.selectAttachments('chat-a', 7)).resolves.toEqual(snapshot);
    await expect(gateway.removeAttachment('chat-a', 8, 'attachment-a')).resolves.toEqual(snapshot);
    const attachment = { id: 'attachment-a', conversationId: 'chat-a', messageId: null, name: 'note.txt', mediaType: 'text/plain', kind: 'text', byteLength: 5, sha256: 'abc', createdAtMs: 123 };
    invokeMock.mockResolvedValueOnce({ attachment, text: 'hello', dataUrl: null });
    await expect(gateway.previewAttachment('chat-a', 'attachment-a')).resolves.toEqual({ attachment, text: 'hello', dataUrl: null });
    expect(invokeMock.mock.calls).toEqual([
      ['crowclaw_attachments_select', { request: { conversationId: 'chat-a', revision: 7 } }],
      ['crowclaw_attachment_remove', { request: { conversationId: 'chat-a', revision: 8, attachmentId: 'attachment-a' } }],
      ['crowclaw_attachment_preview', { request: { conversationId: 'chat-a', attachmentId: 'attachment-a' } }],
    ]);
  });

  it('maps retained draft and sent attachment metadata after recreating the gateway', async () => {
    const attachment = { id: 'file-1', conversationId: 'chat-a', messageId: null, name: 'notes.txt', mediaType: 'text/plain', kind: 'text', byteLength: 32, sha256: 'hash', createdAtMs: 123 };
    const composer = { conversationId: 'chat-a', revision: 9, draft: '', selection: null, attachments: [attachment] };
    invokeMock.mockResolvedValueOnce({ composer, connection: null, sources: [], warning: null });
    expect((await createTauriGateway().getComposer('chat-a')).composer.attachments).toEqual([attachment]);
    const message = { id: 'message-1', role: 'user', content: '', createdAt: new Date(123).toISOString(), status: 'sent', attachments: [{ ...attachment, messageId: 'message-1' }] };
    invokeMock.mockResolvedValueOnce({ id: 'chat-a', messages: [message] });
    expect((await createTauriGateway().getConversation('chat-a')).messages[0]).toEqual(message);
  });

  it('preserves recoverable native attachment errors for UI recovery', async () => {
    invokeMock.mockRejectedValueOnce({ message: 'This model does not support PDF attachments. Choose another model.' });
    await expect(createTauriGateway().selectAttachments('chat-a', 9)).rejects.toThrow('Choose another model');
    invokeMock.mockRejectedValueOnce('Composer revision conflict');
    await expect(createTauriGateway().removeAttachment('chat-a', 9, 'file-1')).rejects.toThrow('Composer revision conflict');
  });

  it("binds composer reads/writes and submission revisions to their conversation", async () => {
    invokeMock.mockResolvedValue(undefined);
    const gateway=createTauriGateway();
    const selection={providerProfileId:'membership:account-a',model:'returned-model',reasoningEffort:'low'};
    await gateway.getComposer('chat-a');
    await gateway.saveComposerDraft('chat-a',3,'unsent');
    await gateway.chooseComposerModel('chat-a',4,selection);
    await gateway.refreshComposerModels(selection.providerProfileId);
    await gateway.sendMessage('chat-a','unsent',null,5);
    expect(invokeMock.mock.calls).toEqual([
      ['crowclaw_composer_get',{request:{conversationId:'chat-a'}}],
      ['crowclaw_composer_save_draft',{request:{conversationId:'chat-a',revision:3,draft:'unsent'}}],
      ['crowclaw_composer_choose',{request:{conversationId:'chat-a',revision:4,selection}}],
      ['crowclaw_composer_refresh_models',{request:{sourceId:'membership:account-a'}}],
      ['crowclaw_chat_send',{request:{conversationId:'chat-a',content:'unsent',selectedFolder:null,composerRevision:5}}],
    ]);
  });

  it("uses explicit, unique CrowClaw command names", () => {
    const commands = Object.values(TAURI_COMMANDS);
    expect(new Set(commands).size).toBe(commands.length);
    expect(commands.every((command) => command.startsWith("crowclaw_"))).toBe(true);
    expect(TAURI_COMMANDS.decideAction).toBe("crowclaw_action_decide");
    expect(TAURI_COMMANDS.cancelTask).toBe("crowclaw_task_cancel");
    expect(TAURI_COMMANDS.listCrowQuantMemories).toBe("crowclaw_crowquant_list");
    expect(TAURI_COMMANDS.rememberCrowQuant).toBe("crowclaw_crowquant_remember");
    expect(TAURI_COMMANDS.recallCrowQuant).toBe("crowclaw_crowquant_recall");
  });

  it("uses the frozen membership RPC envelopes without passing auth material to JavaScript", async () => {
    invokeMock.mockResolvedValue(undefined);
    const gateway = createTauriGateway();
    const signIn = { requestId: "e411b781-4e6b-479c-967b-7b8b4cc78237", label: "Personal", accountId: null };
    const choice = { accountId: "account-1", model: "returned-model", reasoningEffort: null };
    await gateway.membershipSnapshot();
    await gateway.signInMembership(signIn);
    await gateway.cancelMembershipSignIn(signIn.requestId);
    await gateway.signOutMembership("account-1");
    await gateway.refreshMembershipModels("account-1");
    await gateway.useMembershipModel(choice);
    await gateway.acknowledgeMembershipWelcome();
    await gateway.manageMembershipUsage();
    expect(invokeMock.mock.calls).toEqual([
      ["crowclaw_membership_snapshot", undefined],
      ["crowclaw_membership_sign_in", { request: signIn }],
      ["crowclaw_membership_cancel_sign_in", { requestId: signIn.requestId }],
      ["crowclaw_membership_sign_out", { accountId: "account-1" }],
      ["crowclaw_membership_refresh_models", { accountId: "account-1" }],
      ["crowclaw_membership_use_model", { request: choice }],
      ["crowclaw_membership_acknowledge_welcome", undefined],
      ["crowclaw_membership_manage_usage", undefined],
    ]);
  });

  it("routes Codex image sign-in by saved membership account ID only", async () => {
    const gateway=createTauriGateway();
    invokeMock.mockResolvedValue({state:"pending",verificationUrl:"https://auth.openai.com/codex/device",userCode:"ABCD-EFGH",pollIntervalSeconds:5,message:null});
    await gateway.beginCodexImageAuthorization("saved-account");
    await gateway.pollCodexImageAuthorization("saved-account");
    await gateway.codexImageAuthorizationStatus("saved-account");
    await gateway.cancelCodexImageAuthorization("saved-account");
    expect(invokeMock.mock.calls).toEqual([
      ["crowclaw_codex_image_auth_begin",{accountId:"saved-account"}],
      ["crowclaw_codex_image_auth_poll",{accountId:"saved-account"}],
      ["crowclaw_codex_image_auth_status",{accountId:"saved-account"}],
      ["crowclaw_codex_image_auth_cancel",{accountId:"saved-account"}],
    ]);
  });

  it("preserves a false cancellation result and native error messages", async () => {
    const gateway = createTauriGateway();
    invokeMock.mockResolvedValueOnce(false);
    await expect(gateway.cancelMembershipSignIn("request")).resolves.toBe(false);
    invokeMock.mockRejectedValueOnce("Native sign-in failed");
    await expect(gateway.signInMembership({ requestId: "request", label: "Personal", accountId: "account" })).rejects.toThrow("Native sign-in failed");
  });

  it("sends CrowQuant requests in the native command envelope", async () => {
    const gateway = createTauriGateway();
    invokeMock.mockResolvedValue(undefined);

    await gateway.listCrowQuantMemories();
    await gateway.rememberCrowQuant("Remember this");
    await gateway.recallCrowQuant("this", 8);

    expect(invokeMock).toHaveBeenNthCalledWith(1, "crowclaw_crowquant_list", undefined);
    expect(invokeMock).toHaveBeenNthCalledWith(2, "crowclaw_crowquant_remember", {
      request: { text: "Remember this" },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "crowclaw_crowquant_recall", {
      request: { query: "this", limit: 8 },
    });
  });

  it("sends native memory requests without granting any external-file access", async () => {
    invokeMock.mockResolvedValue(undefined);
    const gateway = createTauriGateway();
    const query = { query: "telescope", limit: 5, sourceKind: "conversation_message", mode: "hybrid" as const };
    await gateway.searchMemory(query);
    await gateway.withdrawMemory("source-id");
    await gateway.configureMemory({ indexConversations: false, indexActions: false });
    expect(invokeMock).toHaveBeenNthCalledWith(1, "crowclaw_memory_search", { request: query });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "crowclaw_memory_withdraw", { request: { sourceId: "source-id" } });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "crowclaw_memory_configure", { request: { indexConversations: false, indexActions: false } });
  });
});
