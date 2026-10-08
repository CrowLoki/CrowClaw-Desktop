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
  if (!summary.closest("details")!.open) await userEvent.click(summary);
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
  it("blocks the old composer during another chat load and ignores a superseded load failure", async () => {
    const gateway=createDevelopmentGateway({firstRun:false,delayMs:0});
    await gateway.createConversation();
    const originalGet=gateway.getConversation.bind(gateway);
    let rejectLoad!:(reason:Error)=>void;
    const pending=new Promise<Awaited<ReturnType<typeof gateway.getConversation>>>((_,reject)=>{rejectLoad=reject;});
    vi.spyOn(gateway,'getConversation').mockImplementation(id=>id==='conversation-welcome'?originalGet(id):pending);
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
