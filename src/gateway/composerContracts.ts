import type { ModelConnection } from './contracts';
import type { AttachmentSummary } from './attachmentContracts';

export type ConversationModelChoice = {
  providerProfileId: string;
  model: string;
  reasoningEffort: string | null;
};
export type ConversationComposerState = {
  conversationId: string;
  revision: number;
  draft: string;
  selection: ConversationModelChoice | null;
  /** Older snapshots may omit this field; consumers treat absence as []. */
  attachments?: AttachmentSummary[];
};
export type ComposerModelSource = {
  id: string;
  label: string;
  provider: string;
  status: 'ready' | 'disconnected';
  models: Array<{ id: string; displayName: string; reasoningEfforts: string[] }>;
};
export type ConversationComposerSnapshot = {
  composer: ConversationComposerState;
  connection: ModelConnection | null;
  sources: ComposerModelSource[];
  warning: string | null;
};
