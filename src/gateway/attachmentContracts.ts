/** Native-owned immutable snapshot metadata. Never a browser file path. */
export type AttachmentSummary = {
  id: string;
  conversationId: string;
  messageId: string | null;
  name: string;
  mediaType: string;
  kind: 'text' | 'image' | 'file';
  byteLength: number;
  sha256: string;
  createdAtMs: number;
};

export type AttachmentPreview = {
  attachment: AttachmentSummary;
  text: string | null;
  dataUrl: string | null;
};
