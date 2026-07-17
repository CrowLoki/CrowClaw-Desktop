import {
  ArrowUp,
  FolderPlus,
  LoaderCircle,
  MessageCircle,
  Paperclip,
  ShieldCheck,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import crowClawHead from "../assets/branding/crowclaw-head.webp";
import type {
  AgentTask,
  Conversation,
  ModelConnection,
  SelectedFolder,
} from "../gateway/contracts";
import { MessageBubble } from "./MessageBubble";

type ChatWorkspaceProps = {
  conversation: Conversation | null;
  connection: ModelConnection;
  activeTask: AgentTask | null;
  loading: boolean;
  sending: boolean;
  error: string | null;
  onSelectFolder: () => Promise<SelectedFolder | null>;
  onSend: (content: string, selectedFolder: SelectedFolder | null) => Promise<void>;
};

const starterPrompts = [
  "Help me plan a focused task",
  "Explain something clearly",
  "Inspect a folder I choose",
] as const;

export function ChatWorkspace({
  conversation,
  connection,
  activeTask,
  loading,
  sending,
  error,
  onSelectFolder,
  onSend,
}: ChatWorkspaceProps) {
  const [draft, setDraft] = useState("");
  const [selectedFolder, setSelectedFolder] = useState<SelectedFolder | null>(null);
  const [selectingFolder, setSelectingFolder] = useState(false);
  const transcriptRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    transcriptRef.current?.scrollTo({ top: transcriptRef.current.scrollHeight, behavior: "smooth" });
  }, [conversation?.messages.length]);

  async function chooseFolder() {
    setSelectingFolder(true);
    try {
      setSelectedFolder(await onSelectFolder());
    } finally {
      setSelectingFolder(false);
    }
  }

  async function submit() {
    const content = draft.trim();
    if ((!content && !selectedFolder) || sending || !conversation) return;
    setDraft("");
    const folder = selectedFolder;
    setSelectedFolder(null);
    await onSend(content || `Inspect the selected folder “${folder?.name}”.`, folder);
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submit();
    }
  }

  return (
    <main className="chat-workspace">
      <header className="workspace-header">
        <div>
          <span className="eyebrow">Conversation</span>
          <h1>{conversation?.title ?? "CrowClaw"}</h1>
        </div>
        {activeTask && (
          <button className="task-activity-chip" type="button" aria-label={`Task ${activeTask.status}`}>
            {activeTask.status === "running" && <LoaderCircle className="spin" size={15} />}
            {activeTask.status === "waiting-approval" && <ShieldCheck size={15} />}
            <span>{activeTask.status === "waiting-approval" ? "Waiting for you" : "Working"}</span>
          </button>
        )}
      </header>

      <div className="transcript" ref={transcriptRef} aria-live="polite" aria-busy={loading}>
        {loading && (
          <div className="conversation-loading">
            <span />
            <span />
            <span />
          </div>
        )}
        {!loading && conversation && conversation.messages.length > 0 &&
          conversation.messages.map((message) => <MessageBubble message={message} key={message.id} />)}
        {!loading && conversation && conversation.messages.length === 0 && (
          <section className="new-conversation" aria-labelledby="new-conversation-title">
            <img
              className="new-conversation__emblem"
              src={crowClawHead}
              alt=""
              aria-hidden="true"
            />
            <h2 id="new-conversation-title">What are we working on?</h2>
            <p>CrowClaw can talk things through, complete tasks, and request local actions with your approval.</p>
            <div className="starter-prompts">
              {starterPrompts.map((prompt) => (
                <button type="button" key={prompt} onClick={() => setDraft(prompt)}>{prompt}</button>
              ))}
            </div>
          </section>
        )}
        {!loading && !conversation && (
          <section className="new-conversation">
            <span className="new-conversation__icon"><MessageCircle size={28} /></span>
            <h2>Select or create a conversation</h2>
            <p>Your saved chats will appear in the conversation sidebar.</p>
          </section>
        )}
      </div>

      <div className="composer-zone">
        {error && <div className="inline-error" role="alert">{error}</div>}
        <div className="composer">
          {selectedFolder && (
            <div className="attachment-chip">
              <FolderPlus size={15} />
              <span><strong>{selectedFolder.name}</strong><small>{selectedFolder.displayPath}</small></span>
              <button type="button" onClick={() => setSelectedFolder(null)} aria-label="Remove selected folder"><X size={14} /></button>
            </div>
          )}
          <textarea
            value={draft}
            onChange={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={handleKeyDown}
            placeholder={`Message CrowClaw · ${connection.model}`}
            rows={1}
            disabled={!conversation || sending}
            aria-label="Message CrowClaw"
          />
          <div className="composer__toolbar">
            <button className="composer-tool" type="button" onClick={() => void chooseFolder()} disabled={selectingFolder || sending}>
              {selectingFolder ? <LoaderCircle className="spin" size={17} /> : <Paperclip size={17} />}
              Choose folder
            </button>
            <span className="composer-hint"><ShieldCheck size={14} /> Local actions require permission</span>
            <button className="send-button" type="button" onClick={() => void submit()} disabled={(!draft.trim() && !selectedFolder) || sending || !conversation} aria-label="Send message">
              {sending ? <LoaderCircle className="spin" size={18} /> : <ArrowUp size={19} />}
            </button>
          </div>
        </div>
        <p className="composer-note">Enter to send · Shift + Enter for a new line</p>
      </div>
    </main>
  );
}

