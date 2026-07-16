import { Check, Copy, LoaderCircle, ShieldQuestion } from "lucide-react";
import { useState } from "react";
import type { ConversationMessage } from "../gateway/contracts";

type MessageBubbleProps = {
  message: ConversationMessage;
};

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(new Date(value));
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const [copied, setCopied] = useState(false);
  const assistant = message.role === "assistant";

  async function copyMessage() {
    await navigator.clipboard.writeText(message.content);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  }

  return (
    <article className={assistant ? "message message--assistant" : "message message--user"}>
      <div className="message__avatar" aria-hidden="true">
        {assistant ? <span className="crow-avatar">C</span> : <span className="user-avatar">You</span>}
      </div>
      <div className="message__body">
        <div className="message__meta">
          <strong>{assistant ? "CrowClaw" : "You"}</strong>
          <time dateTime={message.createdAt}>{formatTime(message.createdAt)}</time>
          {message.status === "streaming" && <LoaderCircle className="spin" size={14} aria-label="Responding" />}
          {message.status === "waiting-approval" && (
            <span className="approval-waiting"><ShieldQuestion size={13} /> Approval needed</span>
          )}
        </div>
        <p>{message.content}</p>
        {assistant && message.status !== "streaming" && (
          <button className="message-copy" type="button" onClick={() => void copyMessage()} aria-label="Copy response">
            {copied ? <Check size={14} /> : <Copy size={14} />}
            {copied ? "Copied" : "Copy"}
          </button>
        )}
      </div>
    </article>
  );
}

