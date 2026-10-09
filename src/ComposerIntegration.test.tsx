import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { createDevelopmentGateway } from "./gateway/developmentGateway";

async function ready() {
  const input = await screen.findByRole("textbox", { name: "Message CrowClaw" });
  await waitFor(() => expect(input).toBeEnabled());
  return input;
}

async function chooseModel(sourceId: string, model: string) {
  const summary = screen.getByText("Model & effort");
  if (summary.closest("button")!.getAttribute("aria-expanded") !== "true") await userEvent.click(summary);
  const details=screen.getByText("Selection details");
  if (!details.closest("details")!.open) await userEvent.click(details);
  await waitFor(() => expect(screen.getByLabelText("Provider/account")).toBeEnabled());
  await userEvent.selectOptions(screen.getByLabelText("Provider/account"), sourceId);
  await userEvent.selectOptions(screen.getByLabelText("Model", { exact: true }), model);
  await userEvent.click(screen.getByRole("button", { name: "Use for next message" }));
  await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", `Message CrowClaw · ${model}`));
  await waitFor(() => expect(screen.getByRole("button", { name: "Use for next message" })).toBeEnabled());
}

async function withOllama() {
  const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
  const ollama = await gateway.connectModel({ provider: "ollama", label: "Ollama", baseUrl: "http://127.0.0.1:11434/v1", model: "qwen3.5:9b" });
  return { gateway, ollama };
}

describe("App conversation composer integration", () => {
  it('adds a fixture, previews it, preserves it through model and Settings changes, then removes it', async () => {
    const { gateway, ollama } = await withOllama();
    const id = (await gateway.bootstrap()).selectedConversationId!;
    const folders = vi.spyOn(gateway, 'selectFolder');
    const selects = vi.spyOn(gateway, 'selectAttachments');
    const removes = vi.spyOn(gateway, 'removeAttachment');
    render(<App gateway={gateway} />);
    fireEvent.change(await ready(), { target: { value: 'Keep this text' } });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Add files' })).toBeEnabled());
    await userEvent.click(screen.getByRole('button', { name: 'Add files' }));
    const selected = await screen.findByRole('list', { name: 'Selected files' });
    expect(selects).toHaveBeenCalledWith(id, expect.any(Number));
    expect(folders).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Remove selected folder' })).not.toBeInTheDocument();
    expect(screen.getByText(/Adding files does not grant folder access/)).toBeVisible();
    await userEvent.click(within(selected).getByRole('button', { name: 'Preview Development fixture.txt' }));
    expect(await screen.findByText('Development attachment fixture.')).toBeVisible();
    const savedFiles = (await gateway.getComposer(id)).composer.attachments;
    await chooseModel(ollama.id, 'gemma3:4b');
    expect((await gateway.getComposer(id)).composer.attachments).toEqual(savedFiles);
    expect(screen.getByText(/Selected files go to the selected model \(gemma3:4b\)/)).toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await userEvent.click(screen.getByRole('button', { name: 'Chat' }));
    expect(await ready()).toHaveValue('Keep this text');
    await userEvent.click(screen.getByRole('button', { name: 'Remove Development fixture.txt' }));
    await waitFor(() => expect(screen.queryByRole('list', { name: 'Selected files' })).not.toBeInTheDocument());
    expect(removes).toHaveBeenCalledWith(id, expect.any(Number), savedFiles![0].id);
    expect((await gateway.getComposer(id)).composer.draft).toBe('Keep this text');
  });

  it('sends attachment-only empty content, clears the submitted draft files, and previews the sent snapshot', async () => {
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    const id = (await gateway.bootstrap()).selectedConversationId!;
    const send = vi.spyOn(gateway, 'sendMessage');
    const view = render(<App gateway={gateway} />);
    await ready();
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Add files' }));
    await screen.findByRole('list', { name: 'Selected files' });
    const saved = (await gateway.getComposer(id)).composer;
    expect(screen.getByRole('button', { name: 'Send message' })).toBeEnabled();
    fireEvent.keyDown(screen.getByLabelText('Message CrowClaw'), { key: 'Enter', isComposing: true });
    expect(send).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: 'Send message' }));
    const sent = await screen.findByRole('list', { name: 'Sent attachments' });
    expect(send).toHaveBeenCalledWith(id, '', null, saved.revision);
    await waitFor(() => expect(screen.queryByRole('list', { name: 'Selected files' })).not.toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'Send message' })).toBeDisabled();
    expect(within(sent).queryByRole('button', { name: /Remove/ })).not.toBeInTheDocument();
    await userEvent.click(within(sent).getByRole('button', { name: 'Preview Development fixture.txt' }));
    expect(await screen.findByText('Development attachment fixture.')).toBeVisible();
    const userMessage = (await gateway.getConversation(id)).messages.find(item => item.role === 'user')!;
    expect(userMessage.content).toBe('');
    expect(userMessage.attachments).toEqual([{ ...saved.attachments![0], messageId: userMessage.id }]);
    expect((await gateway.getComposer(id)).composer.attachments).toEqual([]);
    // Re-mount proves the view reads retained gateway metadata; this is not a native restart test.
    view.unmount();
    render(<App gateway={gateway} />);
    await ready();
    expect(screen.getByRole('list', { name: 'Sent attachments' })).toHaveTextContent('Development fixture.txt');
  });

  it('retains selected files and text on a recoverable model failure, then sends on explicit retry', async () => {
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    const id = (await gateway.bootstrap()).selectedConversationId!;
    const send = vi.spyOn(gateway, 'sendMessage').mockRejectedValueOnce(new Error('Selected model does not support this file. Choose another model and retry.'));
    render(<App gateway={gateway} />);
    await ready();
    await userEvent.click(screen.getByRole('button', { name: 'Add files' }));
    await screen.findByRole('list', { name: 'Selected files' });
    fireEvent.change(screen.getByLabelText('Message CrowClaw'), { target: { value: 'Keep my draft' } });
    await userEvent.click(screen.getByRole('button', { name: 'Send message' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Selected model does not support');
    expect(screen.getByLabelText('Message CrowClaw')).toHaveValue('Keep my draft');
    expect(screen.getByRole('list', { name: 'Selected files' })).toHaveTextContent('Development fixture.txt');
    expect((await gateway.getComposer(id)).composer.attachments).toHaveLength(1);
    expect((await gateway.getConversation(id)).messages.filter(item => item.role === 'user')).toHaveLength(0);
    await userEvent.click(screen.getByRole('button', { name: 'Send message' }));
    await screen.findByRole('list', { name: 'Sent attachments' });
    expect(send).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(screen.queryByRole('list', { name: 'Selected files' })).not.toBeInTheDocument());
  });

  it('confines a pending file selection to its original chat through navigation', async () => {
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    const id = (await gateway.bootstrap()).selectedConversationId!;
    const original = gateway.selectAttachments.bind(gateway);
    let finish!: () => Promise<void>;
    vi.spyOn(gateway, 'selectAttachments').mockImplementation((chatId, revision) => new Promise(resolve => {
      finish = async () => resolve(await original(chatId, revision));
    }));
    render(<App gateway={gateway} />);
    await ready();
    await userEvent.click(screen.getByRole('button', { name: 'Add files' }));
    await userEvent.click(screen.getByRole('button', { name: 'New conversation' }));
    await waitFor(() => expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('New conversation'));
    fireEvent.change(await ready(), { target: { value: 'Second chat stays here' } });
    await act(async () => finish());
    expect(screen.queryByRole('list', { name: 'Selected files' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('Message CrowClaw')).toHaveValue('Second chat stays here');
    expect((await gateway.getComposer(id)).composer.attachments).toHaveLength(1);
    await userEvent.click(within(screen.getByRole('complementary', { name: 'Conversations' })).getByRole('button', { name: /Welcome to CrowClaw/ }));
    await ready();
    expect(await screen.findByRole('list', { name: 'Selected files' })).toHaveTextContent('Development fixture.txt');
  });

  it('uses immutable in-memory fixture copies and rejects wrong-chat preview and stale revision removal', async () => {
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    const id = (await gateway.bootstrap()).selectedConversationId!;
    const initial = await gateway.getComposer(id);
    const selected = await gateway.selectAttachments(id, initial.composer.revision);
    const file = selected.composer.attachments![0];
    file.name = 'changed outside gateway';
    expect((await gateway.previewAttachment(id, file.id)).attachment.name).toBe('Development fixture.txt');
    const other = (await gateway.createConversation()).conversation.id;
    await expect(gateway.previewAttachment(other, file.id)).rejects.toThrow('unavailable in this conversation');
    await expect(gateway.removeAttachment(id, initial.composer.revision, file.id)).rejects.toThrow('changed');
    expect((await gateway.getComposer(id)).composer.attachments).toHaveLength(1);
    await gateway.removeAttachment(id, selected.composer.revision, file.id);
    await expect(gateway.previewAttachment(id, file.id)).rejects.toThrow('unavailable');
  });

  it('retains a ninth draft file when retained conversation files exceed the request budget', async () => {
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    const id = (await gateway.bootstrap()).selectedConversationId!;
    let snapshot = await gateway.getComposer(id);
    for (let count = 0; count < 8; count++) snapshot = await gateway.selectAttachments(id, snapshot.composer.revision);
    await expect(gateway.selectAttachments(id, snapshot.composer.revision)).rejects.toThrow('at most 8');
    await gateway.sendMessage(id, '', null, snapshot.composer.revision);
    snapshot = await gateway.getComposer(id);
    snapshot = await gateway.selectAttachments(id, snapshot.composer.revision);
    await expect(gateway.sendMessage(id, '', null, snapshot.composer.revision)).rejects.toThrow('start a new conversation');
    expect((await gateway.getComposer(id)).composer.attachments).toEqual(snapshot.composer.attachments);
    expect((await gateway.getConversation(id)).messages.flatMap(message => message.attachments ?? [])).toHaveLength(8);
  });
  it.each([0, 1000])("blocks the old composer during another chat load and ignores a superseded load failure (timestamp gap %i)", async (timestampGap) => {
    const initialTime = Date.parse('2026-10-09T00:00:00Z');
    const firstTimestamp = new Date(initialTime).toISOString();
    const secondTimestamp = new Date(initialTime + timestampGap).toISOString();
    const timestamp = vi.spyOn(Date.prototype, 'toISOString').mockReturnValue(firstTimestamp);
    const gateway=createDevelopmentGateway({firstRun:false,delayMs:0});
    const initialConversationId = (await gateway.bootstrap()).selectedConversationId!;
    timestamp.mockReturnValue(secondTimestamp);
    await gateway.createConversation();
    timestamp.mockRestore();
    const originalBootstrap = gateway.bootstrap.bind(gateway);
    // This scenario starts in the original chat, independent of recency sorting.
    vi.spyOn(gateway, 'bootstrap').mockImplementation(async () => ({
      ...await originalBootstrap(), selectedConversationId: initialConversationId,
    }));
    const originalGet=gateway.getConversation.bind(gateway);
    let rejectLoad!:(reason:Error)=>void;
    const pending=new Promise<Awaited<ReturnType<typeof gateway.getConversation>>>((_,reject)=>{rejectLoad=reject;});
    vi.spyOn(gateway,'getConversation').mockImplementation(id=>id===initialConversationId?originalGet(id):pending);
    const send=vi.spyOn(gateway,'sendMessage');
    render(<App gateway={gateway}/>);
    fireEvent.change(await ready(),{target:{value:'Keep this in the original chat'}});
    const sidebar=within(screen.getByRole('complementary',{name:'Conversations'}));
    await userEvent.click(sidebar.getByRole('button',{name:/^New conversation\s/}));
    expect(screen.getByLabelText('Message CrowClaw')).toBeDisabled();
    expect(screen.getByRole('button',{name:'Send message'})).toBeDisabled();
    fireEvent.keyDown(screen.getByLabelText('Message CrowClaw'),{key:'Enter'});
    expect(send).not.toHaveBeenCalled();
    await userEvent.click(sidebar.getByRole('button',{name:/Welcome to CrowClaw/}));
    await ready();
    await act(async()=>rejectLoad(new Error('Superseded chat load failed')));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Message CrowClaw')).toHaveValue('Keep this in the original chat');
  });
  it("shows a saved response even if subsequent composer readback fails", async () => {
    const gateway=createDevelopmentGateway({firstRun:false,delayMs:0});
    const originalSend=gateway.sendMessage.bind(gateway);
    vi.spyOn(gateway,'sendMessage').mockImplementation(async (...args)=>{
      const result=await originalSend(...args);
      vi.spyOn(gateway,'getComposer').mockRejectedValueOnce(new Error('Composer receipt unavailable'));
      return result;
    });
    render(<App gateway={gateway}/>);
    fireEvent.change(await ready(),{target:{value:'A completed response must remain visible'}});
    await userEvent.click(screen.getByRole('button',{name:'Send message'}));
    expect(await within(screen.getByRole('main')).findByText(/I’m connected through LM Studio/)).toBeVisible();
    expect(await screen.findByRole('alert')).toHaveTextContent('Response saved, but composer refresh failed');
    expect(screen.getByLabelText('Message CrowClaw')).toHaveValue('A completed response must remain visible');
  });
  it("preserves an unsent draft and selected folder through Settings and back to Chat", async () => {
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    const id = (await gateway.bootstrap()).selectedConversationId!;
    render(<App gateway={gateway} />);
    fireEvent.change(await ready(), { target: { value: "Unsent thought\nSecond line" } });
    await userEvent.click(screen.getByRole("button", { name: "Choose folder" }));
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    expect(await ready()).toHaveValue("Unsent thought\nSecond line");
    expect(screen.getByRole("button", { name: "Remove selected folder" })).toBeEnabled();
    await waitFor(async () => expect((await gateway.getComposer(id)).composer.draft).toBe("Unsent thought\nSecond line"));
  });

  it("keeps independent drafts, folders and model choices when switching chats", async () => {
    const { gateway, ollama } = await withOllama();
    const initial = await gateway.bootstrap();
    const firstId = initial.selectedConversationId!;
    const firstTitle = initial.conversations.find(item => item.id === firstId)!.title;
    render(<App gateway={gateway} />);
    fireEvent.change(await ready(), { target: { value: "First chat draft" } });
    await userEvent.click(screen.getByRole("button", { name: "Choose folder" }));
    await chooseModel(ollama.id, "gemma3:4b");
    await userEvent.click(screen.getByRole("button", { name: "New conversation" }));
    await waitFor(() => expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("New conversation"));
    expect(await ready()).toHaveValue("");
    expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", "Message CrowClaw · qwen3.5:9b");
    expect(screen.queryByRole("button", { name: "Remove selected folder" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Message CrowClaw"), { target: { value: "Second chat draft" } });
    await chooseModel("connection-development", "local-model");
    await userEvent.click(within(screen.getByRole("complementary", { name: "Conversations" })).getByRole("button", { name: new RegExp(firstTitle) }));
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toHaveValue("First chat draft"));
    expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", "Message CrowClaw · gemma3:4b");
    expect(screen.getByRole("button", { name: "Remove selected folder" })).toBeEnabled();
    await userEvent.click(within(screen.getByRole("complementary", { name: "Conversations" })).getByRole("button", { name: /^New conversation\s/ }));
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toHaveValue("Second chat draft"));
    expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", "Message CrowClaw · local-model");
    expect(screen.queryByRole("button", { name: "Remove selected folder" })).not.toBeInTheDocument();
    expect((await gateway.getComposer(firstId)).composer.selection?.model).toBe("gemma3:4b");
  });

  it("applies an explicit next-turn model without losing text or changing the app default, then sends with that choice", async () => {
    const { gateway, ollama } = await withOllama();
    const initial = await gateway.bootstrap();
    const id = initial.selectedConversationId!;
    const send = vi.spyOn(gateway, "sendMessage");
    render(<App gateway={gateway} />);
    fireEvent.change(await ready(), { target: { value: "Hello from this conversation" } });
    await chooseModel(ollama.id, "gemma3:4b");
    expect(screen.getByLabelText("Message CrowClaw")).toHaveValue("Hello from this conversation");
    const saved = (await gateway.getComposer(id)).composer;
    expect(saved.selection).toEqual({ providerProfileId: ollama.id, model: "gemma3:4b", reasoningEffort: null });
    expect(saved.draft).toBe("Hello from this conversation");
    expect((await gateway.bootstrap()).connection).toEqual(initial.connection);
    await userEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(await within(screen.getByRole("main")).findByText(/I’m connected through Ollama using gemma3:4b/)).toBeVisible();
    expect(send).toHaveBeenCalledWith(id, "Hello from this conversation", null, saved.revision);
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toHaveValue(""));
    expect((await gateway.getComposer(id)).composer.draft).toBe("");
    expect((await gateway.bootstrap()).connection).toEqual(initial.connection);
  });

  it("keeps an existing chat choice when the app default changes and seeds a new chat once", async () => {
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    const id = (await gateway.bootstrap()).selectedConversationId!;
    render(<App gateway={gateway} />);
    fireEvent.change(await ready(), { target: { value: "Keep my old choice" } });
    await userEvent.click(screen.getByRole("button", { name: "Connections" }));
    await userEvent.click(await screen.findByText("Ollama", { selector: "strong" }));
    await userEvent.click(screen.getByRole("button", { name: "Test connection" }));
    await screen.findByText(/Connected to Ollama/);
    await userEvent.click(screen.getByRole("button", { name: "Use this connection" }));
    await userEvent.click(screen.getByRole("button", { name: "Chat" }));
    expect(await ready()).toHaveValue("Keep my old choice");
    expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", "Message CrowClaw · local-model");
    expect((await gateway.getComposer(id)).composer.selection?.model).toBe("local-model");
    expect((await gateway.bootstrap()).connection?.model).toBe("qwen3.5:9b");
    await userEvent.click(screen.getByRole("button", { name: "New conversation" }));
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toHaveAttribute("placeholder", "Message CrowClaw · qwen3.5:9b"));
    expect(await ready()).toHaveValue("");
  });

  it("retains the draft and folder after a send failure and submits them on an explicit retry", async () => {
    const gateway = createDevelopmentGateway({ firstRun: false, delayMs: 0 });
    const id = (await gateway.bootstrap()).selectedConversationId!;
    const send = vi.spyOn(gateway, "sendMessage").mockRejectedValueOnce(new Error("Local endpoint disconnected during send"));
    render(<App gateway={gateway} />);
    fireEvent.change(await ready(), { target: { value: "Inspect these notes" } });
    await userEvent.click(screen.getByRole("button", { name: "Choose folder" }));
    await userEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Local endpoint disconnected during send");
    expect(await ready()).toHaveValue("Inspect these notes");
    expect(screen.getByRole("button", { name: "Remove selected folder" })).toBeEnabled();
    expect((await gateway.getComposer(id)).composer.draft).toBe("Inspect these notes");
    expect((await gateway.getConversation(id)).messages.filter(item => item.role === "user")).toHaveLength(0);
    const failedArguments = send.mock.calls[0];
    await userEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    expect(send.mock.calls[1]).toEqual(failedArguments);
    await waitFor(() => expect(screen.getByLabelText("Message CrowClaw")).toHaveValue(""));
    expect(screen.queryByRole("button", { name: "Remove selected folder" })).not.toBeInTheDocument();
    expect((await gateway.getConversation(id)).messages.filter(item => item.role === "user")).toHaveLength(1);
  });
});
