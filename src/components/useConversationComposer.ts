import { useEffect, useMemo, useRef, useSyncExternalStore } from 'react';
import type { CrowClawGateway } from '../gateway/contracts';
import type { ConversationComposerSnapshot, ConversationModelChoice } from '../gateway/composerContracts';
import type { AttachmentSummary } from '../gateway/attachmentContracts';

type View = {
  snapshot: ConversationComposerSnapshot | null;
  draft: string;
  loading: boolean;
  saving: boolean;
  error: string | null;
};

export type ConversationComposerController = View & {
  attachments: AttachmentSummary[];
  selectAttachments(): Promise<void>;
  removeAttachment(attachmentId: string): Promise<void>;
  setDraft(text: string): void;
  choose(selection: ConversationModelChoice): Promise<void>;
  refreshSource(sourceId: string): Promise<void>;
  setHiddenModels(hiddenModelKeys: string[]): Promise<void>;
  /** Explicit recovery: reload native state, retain local edits, unblock writes.
   * Does not save retained text until a subsequent edit, flush, or choice. */
  refresh(): Promise<void>;
  /** Automatic metadata read; never silently resolves a conflicting local edit. */
  sync(): Promise<void>;
  flush(): Promise<number>;
  submitted(conversationId: string, submittedDraft: string): Promise<void>;
};

type Entry = {
  id: string;
  view: View;
  dirty: boolean;
  edits: number;
  blocked: boolean;
  started: boolean;
  reads: number;
  writes: number;
  timer: ReturnType<typeof setTimeout> | null;
  tail: Promise<unknown>;
  sent: { draft: string; edits: number } | null;
};

const empty: View = { snapshot: null, draft: '', loading: false, saving: false, error: null };
const unavailable = () => new Error('Select a conversation and load its composer before continuing.');

// Owned by the mounted hook, scoped to its gateway. No account-global/provider fallback.
function createController(gateway: CrowClawGateway) {
  const entries = new Map<string, Entry>();
  let visibilityRevision=0;
  let latestHiddenModels:string[]=[];
  const visibilitySnapshots=new WeakMap<ConversationComposerSnapshot,number>();
  async function currentVisibility(operation:()=>Promise<ConversationComposerSnapshot>) {
    const started=visibilityRevision;
    const snapshot=await operation();
    const tagged={...snapshot};
    visibilitySnapshots.set(tagged,started);
    return tagged;
  }
  const listeners = new Set<() => void>();
  const entry = (id: string) => {
    let value = entries.get(id);
    if (!value) {
      value = { id, view: { ...empty, loading: true }, dirty: false, edits: 0,
        blocked: false, started: false, reads: 0, writes: 0, timer: null,
        tail: Promise.resolve(), sent: null };
      entries.set(id, value);
    }
    return value;
  };
  function publish(e: Entry, patch: Partial<View> = {}) {
    e.view = { ...e.view, ...patch, loading: e.reads > 0 || !e.started,
      saving: e.writes > 0 || e.timer !== null };
    listeners.forEach(listener => listener());
  }
  function cancelTimer(e: Entry) {
    if (e.timer !== null) clearTimeout(e.timer);
    e.timer = null;
  }
  function queue<T>(e: Entry, kind: 'read' | 'write', operation: () => Promise<T>): Promise<T> {
    if (kind === 'read') e.reads++; else e.writes++;
    publish(e);
    const result = e.tail.then(operation).catch((cause: unknown) => {
      e.blocked = true;
      cancelTimer(e);
      const detail = cause instanceof Error ? cause.message : String(cause);
      const error = new Error(`${detail} Your draft is retained. Refresh the composer, then retry.`);
      publish(e, { error: error.message });
      throw error;
    }).finally(() => {
      if (kind === 'read') e.reads--; else e.writes--;
      publish(e);
    });
    // A failed operation stops writes, but must not poison the recovery queue.
    e.tail = result.catch(() => undefined);
    return result;
  }
  function accept(e: Entry, snapshot: ConversationComposerSnapshot, preserve: boolean) {
    // Reconcile at publication, not in an earlier async continuation: a
    // visibility response can settle between a wrapper and this caller.
    const started=visibilitySnapshots.get(snapshot);
    if (started!==undefined && started!==visibilityRevision) snapshot={...snapshot,hiddenModelKeys:[...latestHiddenModels]};
    if (snapshot.composer.conversationId !== e.id) throw new Error('Composer response belongs to another conversation.');
    const draft = preserve ? e.view.draft : snapshot.composer.draft;
    e.dirty = draft !== snapshot.composer.draft;
    publish(e, { snapshot, draft });
  }
  async function read(e: Entry) {
    const snapshot = await currentVisibility(()=>gateway.getComposer(e.id));
    // Check dirty at completion, including edits made during the native read.
    accept(e, snapshot, e.dirty || e.sent !== null);
    e.blocked = false;
    publish(e, { error: null });
  }
  async function readMetadata(e:Entry) {
    const snapshot=await currentVisibility(()=>gateway.getComposer(e.id));
    const preserve=e.dirty || e.sent!==null;
    const changed=e.view.snapshot!==null && snapshot.composer.revision!==e.view.snapshot.composer.revision;
    if(preserve && changed) {
      e.blocked=true;
      cancelTimer(e);
      publish(e,{error:'This conversation changed while you were editing. Your draft is retained. Refresh the composer, then retry.'});
    }
    accept(e,snapshot,preserve);
  }
  function start(e: Entry) {
    if (e.started) return;
    e.started = true;
    void queue(e, 'read', () => read(e)).catch(() => undefined);
  }
  async function saveLatest(e: Entry): Promise<number> {
    if (e.blocked) throw new Error('Composer writes are paused until you refresh.');
    if (!e.view.snapshot) throw unavailable();
    while (e.dirty) {
      const draft = e.view.draft;
      const snapshot = await currentVisibility(()=>gateway.saveComposerDraft(e.id, e.view.snapshot!.composer.revision, draft));
      accept(e, snapshot, true);
      // A successful save must acknowledge the requested draft. Never spin on a bad reply.
      if (snapshot.composer.draft !== draft) throw new Error('Native composer did not acknowledge the saved draft.');
    }
    return e.view.snapshot.composer.revision;
  }
  function schedule(e: Entry) {
    cancelTimer(e);
    if (!e.blocked) {
      e.timer = setTimeout(() => {
        e.timer = null;
        void queue(e, 'write', () => saveLatest(e)).catch(() => undefined);
      }, 150);
    }
    publish(e);
  }
  function requireEntry(id: string | null) {
    if (id === null) throw unavailable();
    const e = entry(id);
    start(e);
    return e;
  }
  async function changeAttachments(id: string | null, attachmentId?: string) {
    const e = requireEntry(id);
    cancelTimer(e);
    await queue(e, 'write', async () => {
      const revision = await saveLatest(e);
      const before = e.view.snapshot!.composer;
      const snapshot = await currentVisibility(()=>attachmentId === undefined
        ? gateway.selectAttachments(e.id, revision)
        : gateway.removeAttachment(e.id, revision, attachmentId));
      const after = snapshot.composer;
      if ((after.revision !== revision && after.revision !== revision + 1)
        || after.draft !== before.draft
        || after.selection?.providerProfileId !== before.selection?.providerProfileId
        || after.selection?.model !== before.selection?.model
        || after.selection?.reasoningEffort !== before.selection?.reasoningEffort) {
        throw new Error('This conversation changed while selecting files. Refresh to resolve the conflict.');
      }
      accept(e, snapshot, true);
    });
  }
  return {
    entry,
    start,
    activate(e:Entry) {
      if(!e.started)start(e);
      else void queue(e,'read',()=>readMetadata(e)).catch(()=>undefined);
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    actions(id: string | null): Omit<ConversationComposerController, keyof View | 'attachments'> {
      return {
        selectAttachments: () => changeAttachments(id),
        removeAttachment: attachmentId => changeAttachments(id, attachmentId),
        setDraft(text) {
          if (id === null) return;
          const e = requireEntry(id);
          if (text === e.view.draft) return;
          e.edits++;
          e.dirty = true;
          publish(e, { draft: text });
          schedule(e);
        },
        async flush() {
          const e = requireEntry(id);
          cancelTimer(e);
          return queue(e, 'write', async () => {
            const revision = await saveLatest(e);
            e.sent = { draft: e.view.draft, edits: e.edits };
            return revision;
          });
        },
        async choose(selection) {
          const e = requireEntry(id);
          cancelTimer(e);
          await queue(e, 'write', async () => {
            const revision = await saveLatest(e);
            accept(e, await currentVisibility(()=>gateway.chooseComposerModel(e.id, revision, selection)), true);
          });
        },
        async refresh() {
          const e = requireEntry(id);
          cancelTimer(e);
          await queue(e, 'read', () => read(e));
        },
        async sync() {
          const e=requireEntry(id);
          await queue(e,'read',()=>readMetadata(e));
        },
        async refreshSource(sourceId) {
          const e = requireEntry(id);
          await queue(e, 'read', async () => {
            await gateway.refreshComposerModels(sourceId);
            // Catalog refresh is not implicit permission to retry a failed write.
            await readMetadata(e);
          });
        },
        async setHiddenModels(hiddenModelKeys) {
          const saved=await gateway.setHiddenModels(hiddenModelKeys);
          latestHiddenModels=[...saved];
          visibilityRevision++;
          for (const item of entries.values()) {
            if (item.view.snapshot) publish(item,{snapshot:{...item.view.snapshot,hiddenModelKeys:saved}});
          }
        },
        async submitted(conversationId, submittedDraft) {
          const e = requireEntry(conversationId);
          const sent = e.sent;
          const submittedEdits = sent?.draft === submittedDraft ? sent.edits : e.edits;
          cancelTimer(e);
          await queue(e, 'read', async () => {
            const snapshot = await currentVisibility(()=>gateway.getComposer(e.id));
            const newerEdit = e.edits !== submittedEdits || e.view.draft !== submittedDraft;
            accept(e, snapshot, newerEdit);
            e.blocked = false;
            e.sent = null;
            publish(e, { error: null });
            // A newer edit can have raced native Send and failed CAS. Successful
            // submission reconciliation supplies its authoritative next revision.
            if (e.dirty) schedule(e);
          });
        },
      };
    },
  };
}

/** Keep mounted across Settings/Chat navigation. Each chat retains its own
 * cache and queue; outstanding work keeps its original chat and gateway. */
export function useConversationComposer(
  gateway: CrowClawGateway,
  conversationId: string | null,
): ConversationComposerController {
  const controller = useMemo(() => createController(gateway), [gateway]);
  const e = conversationId === null ? null : controller.entry(conversationId);
  const activeEntry=useRef<Entry|null|undefined>(undefined);
  const activationPending=e!==null && activeEntry.current!==e;
  const view = useSyncExternalStore(controller.subscribe, () => e?.view ?? empty, () => empty);
  useEffect(() => { if(activeEntry.current===e)return;activeEntry.current=e;if(e)controller.activate(e); }, [controller, e]);
  const actions = useMemo(() => controller.actions(conversationId), [controller, conversationId]);
  return { ...view, attachments: view.snapshot?.composer.attachments ?? [], loading:view.loading || activationPending, ...actions };
}
