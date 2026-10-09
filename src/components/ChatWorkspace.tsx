import {
  ArrowUp,
  FolderPlus,
  LoaderCircle,
  MessageCircle,
  Paperclip,
  ShieldCheck,
  X,
} from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import crowClawHead from "../assets/branding/crowclaw-head.webp";
import type {
  AgentTask,
  Conversation,
  ModelConnection,
  SelectedFolder,
} from "../gateway/contracts";
import { MessageBubble } from "./MessageBubble";
import { AttachmentPreview } from './AttachmentPreview';
import type { AttachmentSummary } from '../gateway/attachmentContracts';
import type { CrowClawGateway } from '../gateway/contracts';

type ChatWorkspaceProps = {
  conversation: Conversation | null;
  connection: ModelConnection;
  activeTask: AgentTask | null;
  loading: boolean;
  sending: boolean;
  error: string | null;
  membershipUsage?: ReactNode;
  modelControls?: ReactNode;
  draft: string;
  onDraftChange: (draft: string) => void;
  selectedFolder: SelectedFolder | null;
  onSelectedFolderChange: (folder: SelectedFolder | null) => void;
  composerLoading?: boolean;
  attachments?: AttachmentSummary[];
  attachmentsBusy?: boolean;
  developmentPreview?: boolean;
  onSelectAttachments?: () => Promise<void>;
  onRemoveAttachment?: (attachmentId: string) => Promise<void>;
  previewAttachment?: CrowClawGateway['previewAttachment'];
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
  membershipUsage,
  modelControls,
  draft,
  onDraftChange,
  selectedFolder,
  onSelectedFolderChange,
  composerLoading = false,
  attachments = [],
  attachmentsBusy = false,
  developmentPreview = false,
  onSelectAttachments,
  onRemoveAttachment,
  previewAttachment,
  onSelectFolder,
  onSend,
}: ChatWorkspaceProps) {
  const [selectingFolder, setSelectingFolder] = useState(false);
  const transcriptRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    transcriptRef.current?.scrollTo({ top: transcriptRef.current.scrollHeight, behavior: "smooth" });
  }, [conversation?.id, conversation?.messages.length]);

  async function chooseFolder() {
    setSelectingFolder(true);
    try {
      onSelectedFolderChange(await onSelectFolder());
    } finally {
      setSelectingFolder(false);
    }
  }

  async function submit() {
    const content = draft.trim();
    if ((!content && !selectedFolder && attachments.length === 0) || sending || composerLoading || loading || !conversation || connection.status !== "connected") return;
    const folder = selectedFolder;
    try {
      await onSend(content || (folder ? `Inspect the selected folder “${folder.name}”.` : ''), folder);
      onSelectedFolderChange(null);
    } catch {
      // The parent exposes the failure. Keep the draft and chosen folder intact.
    }
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
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
          conversation.messages.map((message) => <MessageBubble message={message} previewAttachment={previewAttachment} key={`${conversation.id}:${message.id}`} />)}
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
                <button type="button" key={prompt} onClick={() => onDraftChange(prompt)}>{prompt}</button>
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
        {modelControls}
        {connection.provider === "chatgpt" && connection.status === "connected" && <div className="membership-plan"><span>Using ChatGPT plan</span>{membershipUsage}</div>}
        {connection.status !== "connected" && <p role="status">Disconnected. Choose a connection in Settings or Connections to continue.</p>}
        {error && <div className="inline-error" role="alert">{error}</div>}
        <div className="composer">
          {attachments.length > 0 && previewAttachment && <ul className="file-attachments" role="list" aria-label="Selected files">
            {attachments.map(attachment => <li key={`${attachment.conversationId}:${attachment.id}`}>
              <AttachmentPreview attachment={attachment} previewAttachment={previewAttachment} onRemove={onRemoveAttachment}
                disabled={attachmentsBusy || sending || composerLoading || loading} />
            </li>)}
          </ul>}
          {selectedFolder && (
            <div className="attachment-chip">
              <FolderPlus size={15} />
              <span><strong>{selectedFolder.name}</strong><small>{selectedFolder.displayPath}</small></span>
              <button type="button" onClick={() => onSelectedFolderChange(null)} aria-label="Remove selected folder"><X size={14} /></button>
            </div>
          )}
          <textarea
            value={draft}
            onChange={(event) => onDraftChange(event.currentTarget.value)}
            onKeyDown={handleKeyDown}
            placeholder={`Message CrowClaw · ${connection.model}`}
            rows={1}
            disabled={!conversation || composerLoading}
            readOnly={sending}
            aria-label="Message CrowClaw"
          />
          <div className="composer__toolbar">
            {onSelectAttachments && <button className="composer-tool" type="button"
              onClick={() => void onSelectAttachments().catch(() => undefined)}
              disabled={attachmentsBusy || sending || composerLoading || loading || !conversation || attachments.length >= 8}
              aria-describedby="attachment-guidance"><Paperclip size={17} aria-hidden="true" /> Add files</button>}
            <button className="composer-tool" type="button" onClick={() => void chooseFolder()} disabled={selectingFolder || sending || composerLoading || loading || !conversation}>
              {selectingFolder ? <LoaderCircle className="spin" size={17} /> : <FolderPlus size={17} />}
              Choose folder
            </button>
            <span className="composer-hint"><ShieldCheck size={14} /> Local actions require permission</span>
            <button className="send-button" type="button" onClick={() => void submit()} disabled={(!draft.trim() && !selectedFolder && attachments.length === 0) || sending || !conversation || composerLoading || loading || connection.status !== "connected"} aria-label="Send message">
              {sending ? <LoaderCircle className="spin" size={18} /> : <ArrowUp size={19} />}
            </button>
          </div>
        </div>
        {onSelectAttachments && <p className="attachment-guidance" id="attachment-guidance">
          {developmentPreview ? 'Development preview: Add files adds an in-memory text fixture; no native picker or local file is accessed. ' : ''}
          Selected files go to the selected model ({connection.model}) when you Send. Adding files does not grant folder access.
          {' '}20 MiB per file; UTF-8 text up to 1 MiB. Each model request supports up to 8 files and 40 MiB total, including retained chat files. Start a new chat if that limit is reached.
          {' '}Text/code, PNG, JPEG, WebP, GIF, PDF, DOCX, PPTX and XLSX, subject to model support.
        </p>}
        <p className="composer-note">Enter to send · Shift + Enter for a new line</p>
      </div>
    </main>
  );
}
