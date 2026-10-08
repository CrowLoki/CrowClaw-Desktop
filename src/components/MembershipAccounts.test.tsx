import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import type { MembershipAccount, ModelConnection } from "../gateway/contracts";
import type { ConversationComposerState, ConversationModelChoice } from "../gateway/composerContracts";
import { createDevelopmentGateway } from "../gateway/developmentGateway";

// jsdom has no top layer. Only emulate dialog visibility; assertions exercise real App state.
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function account(id: string, label = id, efforts = ["careful", "brief"]): MembershipAccount {
  return {
    id, label, identity: { provider: "chatgpt", issuer: "https://issuer.example", subject: "same-subject", clientId: "client", hostId: `host-${id}`, email: "same@example.test" },
    hasCredentials: true, credentialVersion: 1,
    catalog: { accountId: id, models: [{ slug: `model-${id}`, displayName: `Model ${id}`, reasoningEfforts: efforts }], fetchedAtMs: 1 },
    selection: null, createdAtMs: 1, updatedAtMs: 1,
  };
}

function connection(id: string, model = `model-${id}`): ModelConnection {
  return { id: `membership:${id}`, provider: "chatgpt", label: id, model, baseUrl: "", status: "connected", connectedAt: null, latencyMs: null };
}

function fixture(accounts: MembershipAccount[] = [account("A", "Personal"), account("B", "Work")], firstRun = false, acknowledged = true) {
  const gateway = createDevelopmentGateway({ firstRun, delayMs: 0 });
  // Native membership boundary: Settings owns the seed for future chats; saved
  // composers own their choices. The production preview has no membership login.
  const registrations = new Map(accounts.map(item => [item.id, item]));
  const composers = new Map<string, ConversationComposerState>();
  let defaultChoice: ConversationModelChoice | null = null;
  const localComposer = gateway.getComposer.bind(gateway);
  vi.spyOn(gateway, "getComposer").mockImplementation(async (id) => {
    const local = await localComposer(id);
    let composer = composers.get(id);
    if (!composer) {
      composer = { ...local.composer, selection: defaultChoice ? { ...defaultChoice } : local.composer.selection };
      composers.set(id, composer);
    }
    const sources = [...local.sources, ...[...registrations.values()].map(item => ({
      id: `membership:${item.id}`, label: item.label, provider: "chatgpt",
      status: item.hasCredentials ? "ready" as const : "disconnected" as const,
      models: item.hasCredentials && item.catalog?.accountId === item.id
        ? item.catalog.models.map(model => ({ id: model.slug, displayName: model.displayName, reasoningEfforts: model.reasoningEfforts })) : [],
    }))];
    const choice = composer.selection;
    const source = sources.find(item => item.id === choice?.providerProfileId);
    const model = source?.models.find(item => item.id === choice?.model);
    const available = source?.status === "ready" && model && (choice?.reasoningEffort === null || model.reasoningEfforts.includes(choice!.reasoningEffort!));
    const selectedConnection = choice?.providerProfileId.startsWith("membership:")
      ? connection(choice.providerProfileId.slice("membership:".length), choice.model)
      : local.connection;
    return { composer: structuredClone(composer), sources, connection: available ? selectedConnection : null,
      warning: available ? null : "Selected account or model is unavailable. Reconnect or choose an available model." };
  });
  vi.spyOn(gateway, "saveComposerDraft").mockImplementation(async (id, revision, draft) => {
    const { composer } = await gateway.getComposer(id);
    if (composer.revision !== revision) throw new Error("Composer revision changed");
    composers.set(id, { ...composer, draft, revision: revision + 1 });
    return gateway.getComposer(id);
  });
  vi.spyOn(gateway, "chooseComposerModel").mockImplementation(async (id, revision, selection) => {
    const { composer, sources } = await gateway.getComposer(id);
    const source = sources.find(item => item.id === selection.providerProfileId);
    const model = source?.models.find(item => item.id === selection.model);
    if (composer.revision !== revision || source?.status !== "ready" || !model
      || (selection.reasoningEffort !== null && !model.reasoningEfforts.includes(selection.reasoningEffort))) throw new Error("Unavailable composer choice");
    composers.set(id, { ...composer, selection: { ...selection }, revision: revision + 1 });
    return gateway.getComposer(id);
  });
  vi.spyOn(gateway, "refreshComposerModels").mockImplementation(async (id) => {
    const conversationId = composers.keys().next().value;
    if (!conversationId) throw new Error("No composer loaded");
    const source = (await gateway.getComposer(conversationId)).sources.find(item => item.id === id);
    if (!source) throw new Error("Unknown source");
    return source;
  });
  const createConversation = gateway.createConversation.bind(gateway);
  vi.spyOn(gateway, "createConversation").mockImplementation(async () => {
    const created = await createConversation();
    await gateway.getComposer(created.conversation.id);
    return created;
  });
  vi.spyOn(gateway, "membershipSnapshot").mockResolvedValue({ accounts, welcomeAcknowledged: acknowledged });
  vi.spyOn(gateway, "refreshMembershipModels").mockImplementation(async (id) => accounts.find((item) => item.id === id)!);
  vi.spyOn(gateway, "useMembershipModel").mockImplementation(async (request) => {
    // A newly signed-in registration may be supplied by a deferred test reply.
    const refreshed = vi.mocked(gateway.refreshMembershipModels).mock.results.at(-1);
    const returned: MembershipAccount | undefined = refreshed?.type === "return" ? await refreshed.value : undefined;
    if (returned?.id === request.accountId) registrations.set(returned.id, returned);
    defaultChoice = { providerProfileId: `membership:${request.accountId}`, model: request.model, reasoningEffort: request.reasoningEffort };
    return connection(request.accountId, request.model);
  });
  vi.spyOn(gateway, "acknowledgeMembershipWelcome").mockResolvedValue();
  vi.spyOn(gateway, "manageMembershipUsage").mockResolvedValue();
  vi.spyOn(gateway, "signInMembership").mockResolvedValue(accounts[0] ?? account("new"));
  vi.spyOn(gateway, "signOutMembership").mockImplementation(async (id) => {
    const signedOut = { ...registrations.get(id)!, hasCredentials: false, catalog: null, selection: null };
    registrations.set(id, signedOut);
    return { account: signedOut, remoteRevoked: true, detail: "Revocation confirmed by provider." };
  });
  vi.spyOn(gateway, "cancelMembershipSignIn").mockResolvedValue(true);
  return gateway;
}

async function settings() {
  await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
  await waitFor(() => expect(screen.getByLabelText("Saved ChatGPT account")).toBeEnabled());
}

async function selectAccount(id: string) {
  await userEvent.selectOptions(screen.getByLabelText("Saved ChatGPT account"), id);
}

async function useAccount(id: string) {
  await selectAccount(id);
  await waitFor(() => expect(screen.getByLabelText("ChatGPT model (required)")).toBeEnabled());
  await userEvent.selectOptions(screen.getByLabelText("ChatGPT model (required)"), `model-${id}`);
  await userEvent.click(screen.getByRole("button", { name: "Use this model" }));
}

async function newChat() {
  await userEvent.click(screen.getByRole("button", { name: "Chat" }));
  await userEvent.click(screen.getByRole("button", { name: "New conversation" }));
  await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toBeEnabled());
}

describe("Native membership account workflow", () => {
  it("lists same-email registrations separately and mounting has no membership network side effects", async () => {
    const gateway = fixture();
    render(<App gateway={gateway} />);
    await settings();
    const saved = screen.getByLabelText("Saved ChatGPT account");
    expect(within(saved).getAllByRole("option").map((option) => (option as HTMLOptionElement).value)).toEqual(["", "A", "B"]);
    expect(within(saved).getByRole("option", { name: /Personal · same@example.test/ })).toBeVisible();
    expect(within(saved).getByRole("option", { name: /Work · same@example.test/ })).toBeVisible();
    expect(gateway.refreshMembershipModels).not.toHaveBeenCalled();
    expect(gateway.signInMembership).not.toHaveBeenCalled();
    expect(gateway.useMembershipModel).not.toHaveBeenCalled();
    expect(gateway.manageMembershipUsage).not.toHaveBeenCalled();
  });

  it("keeps account B's picker intact when account A's refresh completes late", async () => {
    const gateway = fixture();
    const lateA = deferred<MembershipAccount>();
    vi.mocked(gateway.refreshMembershipModels).mockImplementation((id) => id === "A" ? lateA.promise : Promise.resolve(account("B", "Work", ["deliberate"])));
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await selectAccount("B");
    await userEvent.selectOptions(screen.getByLabelText("ChatGPT model (required)"), "model-B");
    await userEvent.selectOptions(screen.getByLabelText("Reasoning effort"), "deliberate");
    await act(async () => lateA.resolve(account("A", "Personal", ["slow-A"])));
    expect(screen.getByLabelText("Saved ChatGPT account")).toHaveValue("B");
    expect(screen.getByLabelText("ChatGPT model (required)")).toHaveValue("model-B");
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("deliberate");
    expect(screen.queryByRole("option", { name: "slow-A" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Use this model" }));
    expect(gateway.useMembershipModel).toHaveBeenCalledWith({ accountId: "B", model: "model-B", reasoningEffort: "deliberate" });
  });

  it("ignores an older refresh for the same account after switching away and back", async () => {
    const gateway = fixture();
    const old = deferred<MembershipAccount>();
    vi.mocked(gateway.refreshMembershipModels).mockImplementationOnce(() => old.promise);
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await selectAccount("B");
    await selectAccount("A");
    await userEvent.selectOptions(screen.getByLabelText("ChatGPT model (required)"), "model-A");
    const stale = account("A");
    stale.catalog!.models = [{ slug: "obsolete", displayName: "Obsolete", reasoningEfforts: [] }];
    await act(async () => old.resolve(stale));
    expect(screen.getByLabelText("ChatGPT model (required)")).toHaveValue("model-A");
    expect(screen.queryByRole("option", { name: /Obsolete/ })).not.toBeInTheDocument();
  });

  it("shows sign-in in onboarding, saves a separate registration and persistently acknowledges the first-sign-in notice", async () => {
    const gateway = fixture([], true, false);
    const signIn = deferred<MembershipAccount>();
    const saved = account("new", "Personal");
    vi.mocked(gateway.signInMembership).mockReturnValue(signIn.promise);
    vi.mocked(gateway.refreshMembershipModels).mockResolvedValue(saved);
    render(<App gateway={gateway} />);
    const label = await screen.findByLabelText("Registration label (required)");
    await userEvent.type(label, "Personal");
    await userEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    expect(gateway.signInMembership).toHaveBeenCalledWith({ requestId: expect.stringMatching(/^[0-9a-f-]{36}$/), label: "Personal", accountId: null });
    expect(screen.getByRole("button", { name: "Continue with ChatGPT" })).toBeDisabled();
    await userEvent.clear(label);
    await userEvent.type(label, "Next registration");
    await act(async () => signIn.resolve(saved));
    const welcome = await screen.findByRole("dialog", { name: "You're using your ChatGPT plan" });
    expect(gateway.refreshMembershipModels).toHaveBeenCalledWith("new");
    expect(label).toHaveValue("Next registration");
    expect(gateway.acknowledgeMembershipWelcome).not.toHaveBeenCalled();
    await userEvent.click(within(welcome).getByRole("button", { name: "Got it" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(gateway.acknowledgeMembershipWelcome).toHaveBeenCalledTimes(1);
    await selectAccount("new");
    await userEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    await waitFor(() => expect(gateway.signInMembership).toHaveBeenCalledTimes(2));
    expect(gateway.signInMembership).toHaveBeenLastCalledWith(expect.objectContaining({ accountId: "new", label: "Personal" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await useAccount("new");
    expect(await screen.findByRole("navigation", { name: /CrowClaw sections/ })).toBeVisible();
    expect(await screen.findByText("Using ChatGPT plan")).toBeVisible();
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toBeEnabled());
    await userEvent.click(screen.getByRole("button", { name: "Manage usage" }));
    expect(gateway.manageMembershipUsage).toHaveBeenCalledOnce();
  });

  it("retains the first-sign-in dialog if persistent acknowledgement fails", async () => {
    const gateway = fixture([account("A")], false, false);
    vi.mocked(gateway.acknowledgeMembershipWelcome).mockRejectedValueOnce(new Error("Acknowledgement could not be saved"));
    render(<App gateway={gateway} />);
    const welcome = await screen.findByRole("dialog");
    await userEvent.click(within(welcome).getByRole("button", { name: "Got it" }));
    expect(await within(welcome).findByRole("alert")).toHaveTextContent("Acknowledgement could not be saved");
    expect(welcome).toBeVisible();
    await userEvent.click(within(welcome).getByRole("button", { name: "Got it" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("validates a label on submission and surfaces sign-in errors with focus", async () => {
    const gateway = fixture();
    vi.mocked(gateway.signInMembership).mockRejectedValue(new Error("Native sign-in unavailable"));
    render(<App gateway={gateway} />);
    await settings();
    await userEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a label");
    expect(screen.getByRole("alert")).toHaveFocus();
    expect(gateway.signInMembership).not.toHaveBeenCalled();
    await userEvent.type(screen.getByLabelText("Registration label (required)"), "New");
    await userEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Native sign-in unavailable");
    expect(screen.getByRole("button", { name: "Continue with ChatGPT" })).toBeEnabled();
  });

  it("reports a confirmed cancellation and keeps other saved registrations", async () => {
    const gateway = fixture();
    const signIn = deferred<MembershipAccount>();
    vi.mocked(gateway.signInMembership).mockReturnValue(signIn.promise);
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await userEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel sign-in" }));
    await act(async () => signIn.reject(new Error("Cancelled by native service")));
    expect(await screen.findByText("Sign-in cancelled.")).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(gateway.cancelMembershipSignIn).toHaveBeenCalledWith(vi.mocked(gateway.signInMembership).mock.calls[0][0].requestId);
    expect(screen.getByRole("option", { name: /Work · same@example.test/ })).toBeInTheDocument();
  });

  it("does not call a finished sign-in cancelled or discard its newly saved account", async () => {
    const gateway = fixture([]);
    const signIn = deferred<MembershipAccount>();
    const cancelled = deferred<boolean>();
    const saved = account("new", "New registration");
    vi.mocked(gateway.signInMembership).mockReturnValue(signIn.promise);
    vi.mocked(gateway.cancelMembershipSignIn).mockReturnValue(cancelled.promise);
    vi.mocked(gateway.refreshMembershipModels).mockResolvedValue(saved);
    render(<App gateway={gateway} />);
    await settings();
    await userEvent.type(screen.getByLabelText("Registration label (required)"), "New registration");
    await userEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel sign-in" }));
    await act(async () => signIn.resolve(saved));
    await act(async () => cancelled.resolve(false));
    expect(await screen.findByText("Sign-in already finished. Saved accounts are up to date.")).toBeVisible();
    expect(screen.getByRole("option", { name: /New registration · same@example.test/ })).toBeInTheDocument();
    expect(screen.queryByText("Sign-in cancelled.")).not.toBeInTheDocument();
  });

  it("surfaces cancellation failure without stopping the running sign-in", async () => {
    const gateway = fixture();
    const signIn = deferred<MembershipAccount>();
    vi.mocked(gateway.signInMembership).mockReturnValue(signIn.promise);
    vi.mocked(gateway.cancelMembershipSignIn).mockRejectedValue(new Error("Cancellation transport failed"));
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await userEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel sign-in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Cancellation transport failed");
    expect(screen.getByRole("button", { name: "Cancel sign-in" })).toBeEnabled();
    await act(async () => signIn.resolve(account("A")));
    expect(screen.getByLabelText("Saved ChatGPT account")).toHaveValue("A");
  });

  it("uses provider default for an empty effort catalog and seeds a new chat while preserving the old chat", async () => {
    const gateway = fixture([account("A", "Personal", [])]);
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await userEvent.click(screen.getByRole("button", { name: "Use this model" }));
    expect(gateway.useMembershipModel).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a model and reasoning effort");
    await userEvent.selectOptions(screen.getByLabelText("ChatGPT model (required)"), "model-A");
    expect(screen.getByLabelText("Reasoning effort")).toBeDisabled();
    expect(screen.getByRole("option", { name: "Provider default (no override)" })).toBeVisible();
    expect(gateway.useMembershipModel).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Use this model" }));
    expect(gateway.useMembershipModel).toHaveBeenCalledWith({ accountId: "A", model: "model-A", reasoningEffort: null });
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", "Message CrowClaw · local-model"));
    expect(screen.queryByText("Using ChatGPT plan")).not.toBeInTheDocument();
    await newChat();
    expect(await screen.findByText("Using ChatGPT plan")).toBeVisible();
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", "Message CrowClaw · model-A"));
    await userEvent.click(screen.getByRole("button", { name: "Evolution" }));
    expect(await screen.findByText(/model-A/)).toBeVisible();
  });

  it("rejects a mismatched catalog and a disappeared effort after refresh", async () => {
    const gateway = fixture();
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await userEvent.selectOptions(screen.getByLabelText("ChatGPT model (required)"), "model-A");
    await userEvent.selectOptions(screen.getByLabelText("Reasoning effort"), "careful");
    vi.mocked(gateway.refreshMembershipModels).mockResolvedValueOnce(account("A", "Personal", ["brief"]));
    await userEvent.click(screen.getByRole("button", { name: "Refresh models" }));
    await userEvent.click(screen.getByRole("button", { name: "Use this model" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a model and reasoning effort");
    expect(gateway.useMembershipModel).not.toHaveBeenCalled();
    const mismatched = account("A");
    mismatched.catalog!.accountId = "B";
    vi.mocked(gateway.refreshMembershipModels).mockResolvedValueOnce(mismatched);
    await userEvent.click(screen.getByRole("button", { name: "Refresh models" }));
    expect(await screen.findByRole("button", { name: "Use this model" })).toBeDisabled();
  });

  it("preserves newer picker edits while model selection is being saved", async () => {
    const gateway = fixture();
    const save = deferred<ModelConnection>();
    vi.mocked(gateway.useMembershipModel).mockReturnValue(save.promise);
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await userEvent.selectOptions(screen.getByLabelText("ChatGPT model (required)"), "model-A");
    await userEvent.selectOptions(screen.getByLabelText("Reasoning effort"), "careful");
    await userEvent.click(screen.getByRole("button", { name: "Use this model" }));
    await userEvent.selectOptions(screen.getByLabelText("Reasoning effort"), "brief");
    await act(async () => save.resolve(connection("A")));
    expect(screen.getByLabelText("Reasoning effort")).toHaveValue("brief");
    expect(gateway.useMembershipModel).toHaveBeenCalledWith({ accountId: "A", model: "model-A", reasoningEffort: "careful" });
  });

  it("recovers the saved account when cancellation reports finished but the sign-in reply was lost", async () => {
    const gateway = fixture([]);
    const signIn = deferred<MembershipAccount>();
    const cancelled = deferred<boolean>();
    vi.mocked(gateway.signInMembership).mockReturnValue(signIn.promise);
    vi.mocked(gateway.cancelMembershipSignIn).mockReturnValue(cancelled.promise);
    render(<App gateway={gateway} />);
    await settings();
    await userEvent.type(screen.getByLabelText("Registration label (required)"), "Saved despite lost reply");
    await userEvent.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel sign-in" }));
    vi.mocked(gateway.membershipSnapshot).mockResolvedValue({ accounts: [account("recovered", "Recovered")], welcomeAcknowledged: true });
    await act(async () => {
      signIn.reject(new Error("Sign-in reply lost"));
      cancelled.resolve(false);
    });
    expect(await screen.findByRole("option", { name: /Recovered · same@example.test/ })).toBeInTheDocument();
    expect(screen.queryByText("Sign-in cancelled.")).not.toBeInTheDocument();
  });

  it("preserves another active ChatGPT account when signing out a different registration", async () => {
    const gateway = fixture();
    render(<App gateway={gateway} />);
    await settings();
    await useAccount("B");
    await newChat();
    expect(await screen.findByText("Using ChatGPT plan")).toBeVisible();
    await settings();
    await selectAccount("A");
    await userEvent.click(screen.getByRole("button", { name: "Sign out of this account" }));
    expect(await screen.findByText(/Remote access revoked/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toBeEnabled());
    expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", "Message CrowClaw · model-B");
    await userEvent.type(screen.getByLabelText("Message CrowClaw"), "Still using account B");
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
  });

  it("blocks removed model slugs and recovers from refresh errors only after an explicit retry", async () => {
    const gateway = fixture();
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await userEvent.selectOptions(screen.getByLabelText("ChatGPT model (required)"), "model-A");
    const changed = account("A");
    changed.catalog!.models = [{ slug: "new-returned-model", displayName: "New returned model", reasoningEfforts: [] }];
    vi.mocked(gateway.refreshMembershipModels).mockResolvedValueOnce(changed);
    await userEvent.click(screen.getByRole("button", { name: "Refresh models" }));
    await userEvent.click(screen.getByRole("button", { name: "Use this model" }));
    expect(gateway.useMembershipModel).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a model and reasoning effort");
    await userEvent.selectOptions(screen.getByLabelText("ChatGPT model (required)"), "new-returned-model");
    vi.mocked(gateway.refreshMembershipModels).mockRejectedValueOnce(new Error("Catalog request failed"));
    await userEvent.click(screen.getByRole("button", { name: "Refresh models" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Catalog request failed");
    expect(screen.getByRole("button", { name: "Use this model" })).toBeDisabled();
    vi.mocked(gateway.refreshMembershipModels).mockResolvedValueOnce(changed);
    await userEvent.click(screen.getByRole("button", { name: "Refresh models" }));
    expect(screen.getByRole("button", { name: "Use this model" })).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "Use this model" }));
    expect(gateway.useMembershipModel).toHaveBeenCalledWith({ accountId: "A", model: "new-returned-model", reasoningEffort: null });
  });

  it("leaves the provider unchanged when applying a membership model fails", async () => {
    const gateway = fixture();
    vi.mocked(gateway.useMembershipModel).mockRejectedValue(new Error("Model no longer allowed"));
    render(<App gateway={gateway} />);
    await settings();
    await useAccount("A");
    expect(await screen.findByRole("alert")).toHaveTextContent("Model no longer allowed");
    expect(screen.getByRole("button", { name: "Use this model" })).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    expect(screen.getByText("LM Studio", { selector: ".model-chip" })).toBeVisible();
  });

  it("disconnects the active account and reports failed remote revocation without fallback", async () => {
    const gateway = fixture();
    const signOut = vi.mocked(gateway.signOutMembership).getMockImplementation()!;
    vi.mocked(gateway.signOutMembership).mockImplementation(async (id) => ({ ...await signOut(id), remoteRevoked: false, detail: "Provider revocation endpoint unavailable." }));
    const connectLocal = vi.spyOn(gateway, "connectModel");
    const send = vi.spyOn(gateway, "sendMessage");
    render(<App gateway={gateway} />);
    await settings();
    await useAccount("A");
    await newChat();
    expect(await screen.findByText("Using ChatGPT plan")).toBeVisible();
    await userEvent.type(screen.getByLabelText("Message CrowClaw"), "Retain this draft");
    await settings();
    await selectAccount("A");
    await userEvent.click(screen.getByRole("button", { name: "Sign out of this account" }));
    expect(await screen.findByText(/Remote revocation was not confirmed/)).toHaveTextContent("Provider revocation endpoint unavailable.");
    expect(screen.getByRole("option", { name: /Work.*Signed in/ })).toBeInTheDocument();
    expect(connectLocal).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toBeEnabled());
    expect(screen.getByLabelText("Message CrowClaw")).toHaveValue("Retain this draft");
    await userEvent.type(screen.getByLabelText("Message CrowClaw"), " while offline{Enter}");
    expect(screen.getByLabelText("Message CrowClaw")).toHaveValue("Retain this draft while offline");
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(send).not.toHaveBeenCalled();
    expect(connectLocal).not.toHaveBeenCalled();
    expect(screen.queryByText("Using ChatGPT plan")).not.toBeInTheDocument();
    expect(screen.getByText(/Disconnected. Choose a connection/)).toBeVisible();
  });

  it("preserves a local provider when signing out an inactive ChatGPT account", async () => {
    const gateway = fixture();
    render(<App gateway={gateway} />);
    await settings();
    await selectAccount("A");
    await userEvent.click(screen.getByRole("button", { name: "Sign out of this account" }));
    expect(await screen.findByText(/Remote access revoked/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    expect(screen.getByLabelText("Message CrowClaw")).toBeEnabled();
    expect(screen.getByText("LM Studio", { selector: ".model-chip" })).toBeVisible();
  });

  it("keeps local endpoint controls separate when the active provider is ChatGPT", async () => {
    const gateway = fixture();
    render(<App gateway={gateway} />);
    await settings();
    await useAccount("A");
    await userEvent.click(screen.getByRole("button", { name: "Connections" }));
    expect(await screen.findByLabelText("Endpoint URL")).toHaveValue("http://127.0.0.1:1234/v1");
    expect(screen.getAllByRole("radio")).toHaveLength(4);
    expect(screen.queryByRole("radio", { name: /ChatGPT/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Test connection" }));
    await screen.findByText(/Connected to LM Studio/);
    await userEvent.click(screen.getByRole("button", { name: "Use this connection" }));
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    expect(screen.queryByText("Using ChatGPT plan")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Message CrowClaw")).toBeEnabled();
  });

  it("keeps sign-out errors actionable and routes usage failures through native error handling", async () => {
    const gateway = fixture();
    vi.mocked(gateway.signOutMembership).mockRejectedValue(new Error("Credential store unavailable"));
    vi.mocked(gateway.manageMembershipUsage).mockRejectedValue(new Error("Could not open usage settings"));
    render(<App gateway={gateway} />);
    await settings();
    await useAccount("A");
    await newChat();
    expect(await screen.findByText("Using ChatGPT plan")).toBeVisible();
    await settings();
    await selectAccount("A");
    await userEvent.click(screen.getByRole("button", { name: "Sign out of this account" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Credential store unavailable");
    expect(screen.getByRole("button", { name: "Sign out of this account" })).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toBeEnabled());
    await userEvent.click(screen.getByRole("button", { name: "Manage usage" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not open usage settings");
  });

  it("supports keyboard form submission without sending credentials or invented model values", async () => {
    const gateway = fixture();
    render(<App gateway={gateway} />);
    await settings();
    const label = screen.getByLabelText("Registration label (required)");
    await userEvent.type(label, "Keyboard registration");
    fireEvent.submit(label.closest("form")!);
    await waitFor(() => expect(gateway.signInMembership).toHaveBeenCalledOnce());
    expect(Object.keys(vi.mocked(gateway.signInMembership).mock.calls[0][0]).sort()).toEqual(["accountId", "label", "requestId"]);
  });
});
