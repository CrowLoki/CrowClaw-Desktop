import { lazy, Suspense, useEffect, useId, useState } from 'react';
import { Paperclip, X } from 'lucide-react';
import type { AttachmentPreview as Preview, AttachmentSummary } from '../gateway/attachmentContracts';
import type { CrowClawGateway } from '../gateway/contracts';
import './AttachmentPreview.css';

const PdfAttachmentPreview = lazy(() => import('./PdfAttachmentPreview'));

type Props = {
  attachment: AttachmentSummary;
  previewAttachment: CrowClawGateway['previewAttachment'];
  onRemove?: (attachmentId: string) => Promise<void>;
  disabled?: boolean;
};

export function attachmentSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

// Native code validates the bytes. The view additionally admits only explicitly
// supported data URL media types, never HTML, SVG, remote URLs or local paths.
function visualUrl(preview: Preview): string | null {
  const { attachment, dataUrl } = preview;
  const allowed = attachment.kind === 'image'
    ? ['image/png', 'image/jpeg', 'image/webp', 'image/gif']
    : attachment.kind === 'file' ? ['application/pdf'] : [];
  if (!dataUrl || !allowed.includes(attachment.mediaType)) return null;
  const prefix = `data:${attachment.mediaType};base64,`;
  if (!dataUrl.startsWith(prefix) || !/^[A-Za-z0-9+/]+={0,2}$/.test(dataUrl.slice(prefix.length))) return null;
  return dataUrl;
}

export function AttachmentPreview({ attachment, previewAttachment, onRemove, disabled = false }: Props) {
  const panelId = useId();
  const [expanded, setExpanded] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [imageFailed, setImageFailed] = useState(false);

  useEffect(() => {
    if (!expanded) return;
    let active = true;
    setPreview(null);
    setError(null);
    setImageFailed(false);
    void previewAttachment(attachment.conversationId, attachment.id).then(value => {
      if (!active) return;
      if (value.attachment.id !== attachment.id || value.attachment.conversationId !== attachment.conversationId
        || value.attachment.sha256 !== attachment.sha256 || value.attachment.mediaType !== attachment.mediaType
        || value.attachment.kind !== attachment.kind) {
        throw new Error('The preview does not match this attachment. Close it and retry.');
      }
      setPreview(value);
    }).catch((cause: unknown) => {
      if (active) setError(cause instanceof Error ? cause.message : String(cause));
    });
    return () => { active = false; };
  }, [expanded, attempt, attachment.conversationId, attachment.id, attachment.sha256, attachment.mediaType, attachment.kind, previewAttachment]);

  const url = preview ? visualUrl(preview) : null;
  return (
    <div className="file-attachment">
      <div className="file-attachment__chip">
        <Paperclip size={15} aria-hidden="true" />
        <span className="file-attachment__name">{attachment.name}</span>
        <small>{attachmentSize(attachment.byteLength)}</small>
        <button type="button" aria-label={`Preview ${attachment.name}`} aria-expanded={expanded}
          aria-controls={panelId} onClick={() => setExpanded(value => !value)}>
          {expanded ? 'Close preview' : 'Preview'}
        </button>
        {onRemove && <button type="button" disabled={disabled} aria-label={`Remove ${attachment.name}`}
          onClick={() => void onRemove(attachment.id).catch(() => undefined)}><X size={15} aria-hidden="true" /></button>}
      </div>
      {expanded && <section id={panelId} className="file-attachment__preview" aria-label={`Attachment preview: ${attachment.name}`}>
        <p>{attachment.name} · {attachment.mediaType} · {attachmentSize(attachment.byteLength)}</p>
        {error ? <div role="alert">{error} <button type="button" onClick={() => setAttempt(value => value + 1)}>Retry preview</button></div>
          : !preview ? <p role="status">Loading attachment preview…</p>
          : preview.attachment.kind === 'text' && preview.text !== null ? <pre>{preview.text}</pre>
          : url && preview.attachment.kind === 'image' && !imageFailed ? <img src={url} alt={attachment.name} onError={() => setImageFailed(true)} />
          : url && preview.attachment.mediaType === 'application/pdf' ? <Suspense fallback={<p role="status">Loading PDF viewer…</p>}>
              <PdfAttachmentPreview dataUrl={url} name={attachment.name} />
            </Suspense>
          : <p>Visual preview unavailable for this file. Its metadata is shown above.</p>}
      </section>}
    </div>
  );
}
