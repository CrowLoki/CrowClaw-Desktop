import { Check, Copy, LoaderCircle, ShieldQuestion } from "lucide-react";
import { useState } from "react";
import crowClawMark from "../assets/branding/crowclaw-mark.webp";
import type { ConversationMessage } from "../gateway/contracts";
import type { CrowClawGateway } from '../gateway/contracts';
import { AttachmentPreview } from './AttachmentPreview';

type MessageBubbleProps = {
  message: ConversationMessage;
  previewAttachment?: CrowClawGateway['previewAttachment'];
};

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(new Date(value));
}

export function MessageBubble({ message, previewAttachment }: MessageBubbleProps) {
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
        {assistant ? (
          <span className="crow-avatar"><img src={crowClawMark} alt="" /></span>
        ) : (
          <span className="user-avatar">You</span>
        )}
      </div>
      <div className="message__body">
        <div className="message__meta">
          <strong>{assistant ? "CrowClaw" : "You"}</strong>
          <time dateTime={message.createdAt}>{formatTime(message.createdAt)}</time>
          {assistant && message.requestedModel && <small title={`Requested: ${message.requestedModel}; reported: ${message.reportedModel ?? 'not supplied'}`}>
            {message.reportedModel ?? message.requestedModel}{message.reasoningEffort ? ` · ${message.reasoningEffort}` : ''}
            {!message.reportedModel && ' · requested'}
          </small>}
          {message.status === "streaming" && <LoaderCircle className="spin" size={14} aria-label="Responding" />}
          {message.status === "waiting-approval" && (
            <span className="approval-waiting"><ShieldQuestion size={13} /> Approval needed</span>
          )}
        </div>
        <p>{message.content}</p>
        {!!message.attachments?.length && previewAttachment && <ul className="file-attachments" role="list" aria-label="Sent attachments">
          {message.attachments.map(attachment => <li key={attachment.id}><AttachmentPreview attachment={attachment} previewAttachment={previewAttachment} autoPreview={message.role === 'assistant' && attachment.kind === 'image'} /></li>)}
        </ul>}
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

