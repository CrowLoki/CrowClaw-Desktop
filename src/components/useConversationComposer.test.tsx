import { StrictMode, type PropsWithChildren } from 'react';
import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CrowClawGateway } from '../gateway/contracts';
import type { ConversationComposerSnapshot, ConversationModelChoice } from '../gateway/composerContracts';
import { useConversationComposer } from './useConversationComposer';
import type { AttachmentSummary } from '../gateway/attachmentContracts';

const attachment: AttachmentSummary = { id: 'file-a', conversationId: 'a', messageId: null, name: 'note.txt', mediaType: 'text/plain', kind: 'text', byteLength: 4, sha256: 'hash', createdAtMs: 1 };

const selection: ConversationModelChoice = { providerProfileId: 'local', model: 'model-two', reasoningEffort: null };
function snapshot(id: string, draft = '', revision = 10): ConversationComposerSnapshot {
  return { composer: { conversationId: id, revision, draft, selection: null },
    connection: null, sources: [], warning: 'Choose a conversation model.' };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function native() {
  const states = new Map([['a', snapshot('a')], ['b', snapshot('b', 'B saved', 50)]]);
  const getComposer = vi.fn(async (id: string) => structuredClone(states.get(id)!));
  const saveComposerDraft = vi.fn(async (id: string, revision: number, draft: string) => {
    const current = states.get(id)!;
    if (current.composer.revision !== revision) throw new Error('Composer revision conflict');
    const next = { ...current, composer: { ...current.composer, draft, revision: revision + 7 } };
    states.set(id, next);
    return structuredClone(next);
  });
  const chooseComposerModel = vi.fn(async (id: string, revision: number, choice: ConversationModelChoice): Promise<ConversationComposerSnapshot> => {
    const current = states.get(id)!;
    if (current.composer.revision !== revision) throw new Error('Composer revision conflict');
    const next = { ...current, composer: { ...current.composer, selection: choice, revision: revision + 3 } };
    states.set(id, next);
    return structuredClone(next);
  });
  const refreshComposerModels = vi.fn(async (id: string) => ({ id, label: id, provider: 'ollama',
    status: 'ready' as const, models: [] }));
  const selectAttachments = vi.fn(async (id: string, revision: number): Promise<ConversationComposerSnapshot> => {
    const current = states.get(id)!;
    if (current.composer.revision !== revision) throw new Error('Composer revision conflict');
    const next = { ...current, composer: { ...current.composer, revision: revision + 1, attachments: [...(current.composer.attachments ?? []), { ...attachment, conversationId: id }] } };
    states.set(id, next);
    return structuredClone(next);
  });
  const removeAttachment = vi.fn(async (id: string, revision: number, attachmentId: string) => {
    const current = states.get(id)!;
    if (current.composer.revision !== revision) throw new Error('Composer revision conflict');
    const next = { ...current, composer: { ...current.composer, revision: revision + 1, attachments: (current.composer.attachments ?? []).filter(item => item.id !== attachmentId) } };
    states.set(id, next);
    return structuredClone(next);
  });
  const gateway = { getComposer, saveComposerDraft, chooseComposerModel, refreshComposerModels, selectAttachments, removeAttachment } as unknown as CrowClawGateway;
  return { gateway, states, getComposer, saveComposerDraft, chooseComposerModel, refreshComposerModels, selectAttachments, removeAttachment };
}
async function settle() { await act(async () => {}); }
async function autosave() { await act(async () => { await vi.advanceTimersByTimeAsync(150); }); }

describe('useConversationComposer', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('does not adopt a cancelled picker reply that would overwrite another window draft', async () => {
    const n = native();
    n.states.set('a', snapshot('a', 'original', 10));
    const pending = deferred<ConversationComposerSnapshot>();
    n.selectAttachments.mockImplementationOnce(() => pending.promise);
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    let selecting!: Promise<void>;
    act(() => { selecting = result.current.selectAttachments(); });
    await settle();
    const replacement = snapshot('a', 'replacement from another window', 11);
    n.states.set('a', replacement);
    await act(async () => { pending.resolve(replacement); await expect(selecting).rejects.toThrow(/changed|conflict/i); });
    expect(result.current.draft).toBe('original');
    await act(async () => { await expect(result.current.flush()).rejects.toThrow(/paused|refresh/i); });
    expect(n.saveComposerDraft).not.toHaveBeenCalled();
    expect(n.states.get('a')?.composer.draft).toBe('replacement from another window');
  });

  it('treats old snapshots as empty and flushes unsaved edits before attachment CAS, remove and model choice', async () => {
    const n = native();
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    expect(result.current.attachments).toEqual([]);
    act(() => result.current.setDraft('unsaved text'));
    await act(async () => result.current.selectAttachments());
    expect(n.saveComposerDraft).toHaveBeenCalledWith('a', 10, 'unsaved text');
    expect(n.selectAttachments).toHaveBeenCalledWith('a', 17);
    expect(result.current.attachments).toEqual([attachment]);
    await act(async () => result.current.choose(selection));
    expect(n.chooseComposerModel).toHaveBeenCalledWith('a', 18, selection);
    expect(result.current.attachments).toEqual([attachment]);
    act(() => result.current.setDraft('edit before remove'));
    await act(async () => result.current.removeAttachment(attachment.id));
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 21, 'edit before remove');
    expect(n.removeAttachment).toHaveBeenCalledWith('a', 28, attachment.id);
    expect(result.current.attachments).toEqual([]);
    expect(result.current.draft).toBe('edit before remove');
  });

  it('serializes pending selection, model choice and removal, and confines late selection to its chat', async () => {
    const n = native();
    const pending = deferred<ConversationComposerSnapshot>();
    n.selectAttachments.mockImplementationOnce(() => pending.promise as ReturnType<typeof n.selectAttachments>);
    const { result, rerender } = renderHook(({ id }) => useConversationComposer(n.gateway, id), { initialProps: { id: 'a' } });
    await settle();
    let selecting!: Promise<void>;
    let choosing!: Promise<void>;
    let removing!: Promise<void>;
    act(() => {
      selecting = result.current.selectAttachments();
      choosing = result.current.choose(selection);
      removing = result.current.removeAttachment(attachment.id);
    });
    await settle();
    expect(n.chooseComposerModel).not.toHaveBeenCalled();
    act(() => result.current.setDraft('typed during picker'));
    rerender({ id: 'b' });
    await settle();
    expect(result.current.attachments).toEqual([]);
    const next = snapshot('a', '', 11);
    next.composer.attachments = [attachment];
    n.states.set('a', next);
    await act(async () => { pending.resolve(next); await selecting; await choosing; await removing; });
    expect(result.current.draft).toBe('B saved');
    expect(result.current.attachments).toEqual([]);
    expect(n.chooseComposerModel).toHaveBeenCalledWith('a', 18, selection);
    expect(n.removeAttachment).toHaveBeenCalledWith('a', 21, attachment.id);
    rerender({ id: 'a' });
    await settle();
    expect(result.current.draft).toBe('typed during picker');
    expect(result.current.attachments).toEqual([]);
    await autosave();
  });

  it('does not guess attachment state on cancellation or conflict, then recovers explicitly', async () => {
    const n = native();
    const a = snapshot('a');
    a.composer.attachments = [attachment];
    n.states.set('a', a);
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    n.selectAttachments.mockResolvedValueOnce(structuredClone(a) as Awaited<ReturnType<typeof n.selectAttachments>>);
    await act(async () => result.current.selectAttachments());
    expect(result.current.attachments).toEqual([attachment]);
    expect(result.current.snapshot?.composer.revision).toBe(10);
    n.states.set('a', { ...a, composer: { ...a.composer, revision: 30 } });
    await act(async () => { await expect(result.current.removeAttachment(attachment.id)).rejects.toThrow('revision conflict'); });
    expect(result.current.attachments).toEqual([attachment]);
    await act(async () => { await expect(result.current.selectAttachments()).rejects.toThrow('paused'); });
    await act(async () => result.current.refresh());
    await act(async () => result.current.removeAttachment(attachment.id));
    expect(n.removeAttachment).toHaveBeenLastCalledWith('a', 30, attachment.id);
    expect(result.current.attachments).toEqual([]);
  });

  it('keeps files on a failed selection and loads retained files when the hook remounts', async () => {
    const n = native();
    const a = snapshot('a', 'retained draft');
    a.composer.attachments = [attachment];
    n.states.set('a', a);
    const first = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    n.selectAttachments.mockRejectedValueOnce(new Error('Unsupported file type'));
    await act(async () => { await expect(first.result.current.selectAttachments()).rejects.toThrow('Unsupported file type'); });
    expect(first.result.current.attachments).toEqual([attachment]);
    expect(first.result.current.draft).toBe('retained draft');
    first.unmount();
    const second = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    expect(second.result.current.attachments).toEqual([attachment]);
    expect(second.result.current.draft).toBe('retained draft');
  });

  it('uses authoritative post-send attachments and retains newer text instead of clearing all files', async () => {
    const n = native();
    const a = snapshot('a');
    a.composer.attachments = [attachment];
    n.states.set('a', a);
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    await act(async () => result.current.flush());
    act(() => result.current.setDraft('next draft'));
    const pendingAttachment = { ...attachment, id: 'next-file', name: 'next.txt' };
    const next = snapshot('a', '', 50);
    next.composer.attachments = [pendingAttachment];
    n.states.set('a', next);
    await act(async () => result.current.submitted('a', ''));
    expect(result.current.attachments).toEqual([pendingAttachment]);
    expect(result.current.draft).toBe('next draft');
    await autosave();
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 50, 'next draft');
    expect(result.current.attachments).toEqual([pendingAttachment]);
  });

  it('isolates deferred loads and saves across chats and retains the cache on return', async () => {
    const n = native();
    const loadA = deferred<ConversationComposerSnapshot>();
    const saveA = deferred<ConversationComposerSnapshot>();
    n.getComposer.mockImplementationOnce(() => loadA.promise);
    n.saveComposerDraft.mockImplementationOnce(() => saveA.promise);
    const { result, rerender } = renderHook(({ id }) => useConversationComposer(n.gateway, id), { initialProps: { id: 'a' } });
    await settle();
    act(() => result.current.setDraft('A local'));
    rerender({ id: 'b' });
    await settle();
    expect(result.current.draft).toBe('B saved');
    await act(async () => loadA.resolve(snapshot('a', 'A old')));
    await autosave();
    expect(n.saveComposerDraft).toHaveBeenCalledWith('a', 10, 'A local');
    expect(result.current.snapshot?.composer.conversationId).toBe('b');
    act(() => result.current.setDraft('B new'));
    await act(async () => saveA.resolve(snapshot('a', 'A local', 17)));
    expect(result.current.draft).toBe('B new');
    rerender({ id: 'a' });
    expect(result.current.draft).toBe('A local');
    expect(result.current.snapshot?.composer.revision).toBe(17);
    expect(n.getComposer).toHaveBeenCalledTimes(2);
    await autosave();
  });

  it('coalesces rapid edits and saves the newest edit arriving during an outstanding save', async () => {
    const n = native();
    const first = deferred<ConversationComposerSnapshot>();
    n.saveComposerDraft.mockImplementationOnce(() => first.promise);
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => { result.current.setDraft('one'); result.current.setDraft('two'); result.current.setDraft('three'); });
    expect(result.current.draft).toBe('three');
    expect(result.current.saving).toBe(true);
    await autosave();
    expect(n.saveComposerDraft).toHaveBeenCalledTimes(1);
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 10, 'three');
    act(() => result.current.setDraft('newest'));
    n.states.set('a', snapshot('a', 'three', 41));
    await act(async () => first.resolve(n.states.get('a')!));
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 41, 'newest');
    expect(result.current.draft).toBe('newest');
    expect(result.current.snapshot?.composer.revision).toBe(48);
    await autosave();
    expect(result.current.saving).toBe(false);
  });

  it('flushes before choose, waits for native success, and keeps edits made during choose', async () => {
    const n = native();
    const choosing = deferred<ConversationComposerSnapshot>();
    n.chooseComposerModel.mockImplementationOnce(() => choosing.promise);
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('keep me'));
    let finished = false;
    let chosen!: Promise<void>;
    act(() => { chosen = result.current.choose(selection).then(() => { finished = true; }); });
    await settle();
    expect(n.saveComposerDraft).toHaveBeenCalledWith('a', 10, 'keep me');
    expect(n.chooseComposerModel).toHaveBeenCalledWith('a', 17, selection);
    expect(finished).toBe(false);
    act(() => result.current.setDraft('new edit'));
    const next = snapshot('a', 'keep me', 20);
    next.composer.selection = selection;
    n.states.set('a', next);
    await act(async () => { choosing.resolve(next); await chosen; });
    expect(finished).toBe(true);
    expect(result.current.draft).toBe('new edit');
    await autosave();
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 20, 'new edit');
  });

  it('retains failed drafts, blocks further writes and choose, and explicitly recovers at the native revision', async () => {
    const n = native();
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    n.states.set('a', snapshot('a', 'remote edit', 80));
    act(() => result.current.setDraft('local edit'));
    await autosave();
    expect(result.current.error).toMatch(/revision conflict.*draft is retained.*Refresh/);
    expect(result.current.draft).toBe('local edit');
    act(() => result.current.setDraft('new local edit'));
    await autosave();
    await act(async () => { await expect(result.current.flush()).rejects.toThrow(/paused/); });
    await act(async () => { await expect(result.current.choose(selection)).rejects.toThrow(/paused/); });
    expect(n.saveComposerDraft).toHaveBeenCalledTimes(1);
    expect(n.chooseComposerModel).not.toHaveBeenCalled();
    await act(async () => result.current.refresh());
    expect(result.current.error).toBeNull();
    expect(result.current.draft).toBe('new local edit');
    expect(result.current.snapshot?.composer.revision).toBe(80);
    expect(n.saveComposerDraft).toHaveBeenCalledTimes(1);
    await act(async () => { expect(await result.current.flush()).toBe(87); });
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 80, 'new local edit');
  });

  it('flush waits for all latest text and returns only the native revision for Send', async () => {
    const n = native();
    const saving = deferred<ConversationComposerSnapshot>();
    n.saveComposerDraft.mockImplementationOnce(() => saving.promise);
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('sent text'));
    let revision: number | undefined;
    let flushing!: Promise<void>;
    act(() => { flushing = result.current.flush().then(value => { revision = value; }); });
    await settle();
    expect(revision).toBeUndefined();
    act(() => result.current.setDraft('latest sent text'));
    n.states.set('a', snapshot('a', 'sent text', 90));
    await act(async () => { saving.resolve(n.states.get('a')!); await flushing; });
    expect(revision).toBe(97);
    expect(n.states.get('a')?.composer.draft).toBe('latest sent text');
    await autosave();
  });

  it('refreshes sources and connection without replacing edits made during the read', async () => {
    const n = native();
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    const reading = deferred<ConversationComposerSnapshot>();
    n.getComposer.mockImplementationOnce(() => reading.promise);
    let refreshing!: Promise<void>;
    act(() => { refreshing = result.current.refreshSource('local'); });
    await settle();
    act(() => result.current.setDraft('typing during refresh'));
    const next = snapshot('a', 'remote text', 43);
    next.sources = [{ id: 'local', label: 'Local', provider: 'ollama', status: 'disconnected', models: [] }];
    next.warning = 'Selected source disconnected';
    n.states.set('a', next);
    await act(async () => { reading.resolve(next); await refreshing; });
    expect(result.current.snapshot).toEqual(next);
    expect(result.current.snapshot?.connection).toBeNull();
    expect(result.current.draft).toBe('typing during refresh');
    await autosave();
    expect(n.saveComposerDraft).not.toHaveBeenCalled();
    expect(result.current.error).toContain('changed while you were editing');
    await act(async()=>result.current.refresh());
    await act(async()=>result.current.flush());
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 43, 'typing during refresh');
  });

  it('serializes refresh behind a save instead of letting an older read overwrite its revision', async () => {
    const n = native();
    const saving = deferred<ConversationComposerSnapshot>();
    n.saveComposerDraft.mockImplementationOnce(() => saving.promise);
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('draft'));
    await autosave();
    let refreshing!: Promise<void>;
    act(() => { refreshing = result.current.refresh(); });
    await settle();
    expect(n.getComposer).toHaveBeenCalledTimes(1);
    n.states.set('a', snapshot('a', 'draft', 17));
    await act(async () => { saving.resolve(n.states.get('a')!); await refreshing; });
    expect(result.current.snapshot?.composer.revision).toBe(17);
    expect(result.current.draft).toBe('draft');
  });

  it('reconciles submission for the passed chat while another chat is active', async () => {
    const n = native();
    const { result, rerender } = renderHook(({ id }) => useConversationComposer(n.gateway, id), { initialProps: { id: 'a' } });
    await settle();
    act(() => result.current.setDraft('send A'));
    await act(async () => { await result.current.flush(); });
    rerender({ id: 'b' });
    await settle();
    act(() => result.current.setDraft('keep B'));
    n.states.set('a', snapshot('a', '', 25));
    await act(async () => result.current.submitted('a', 'send A'));
    expect(result.current.draft).toBe('keep B');
    expect(result.current.snapshot?.composer.conversationId).toBe('b');
    rerender({ id: 'a' });
    expect(result.current.draft).toBe('');
    expect(result.current.snapshot?.composer.revision).toBe(25);
    await autosave();
  });

  it('does not clear a newer edit even when it returns to the submitted text (ABA)', async () => {
    const n = native();
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('same words'));
    await act(async () => { await result.current.flush(); });
    act(() => { result.current.setDraft('changed'); result.current.setDraft('same words'); });
    n.states.set('a', snapshot('a', '', 33));
    await act(async () => result.current.submitted('a', 'same words'));
    expect(result.current.draft).toBe('same words');
    expect(result.current.snapshot?.composer.revision).toBe(33);
    await autosave();
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 33, 'same words');
  });

  it('preserves a newer edit arriving during submission readback', async () => {
    const n = native();
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('submitted'));
    await act(async () => { await result.current.flush(); });
    const reading = deferred<ConversationComposerSnapshot>();
    n.getComposer.mockImplementationOnce(() => reading.promise);
    let submitted!: Promise<void>;
    act(() => { submitted = result.current.submitted('a', 'submitted'); });
    await settle();
    act(() => result.current.setDraft('next message'));
    n.states.set('a', snapshot('a', '', 30));
    await act(async () => { reading.resolve(n.states.get('a')!); await submitted; });
    expect(result.current.draft).toBe('next message');
    await autosave();
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 30, 'next message');
  });

  it('retains text and rejects choose on a native failure', async () => {
    const n = native();
    n.chooseComposerModel.mockRejectedValueOnce(new Error('Model disconnected'));
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('do not lose'));
    await act(async () => { await expect(result.current.choose(selection)).rejects.toThrow('Model disconnected'); });
    expect(result.current.draft).toBe('do not lose');
    expect(result.current.error).toContain('Model disconnected');
    expect(result.current.snapshot?.composer.selection).toBeNull();
    expect(result.current.saving).toBe(false);
  });

  it('keeps the newest local text on save rejection and does not unblock through catalog refresh', async () => {
    const n = native();
    const saving = deferred<ConversationComposerSnapshot>();
    n.saveComposerDraft.mockImplementationOnce(() => saving.promise);
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('first'));
    await autosave();
    act(() => result.current.setDraft('latest unsaved'));
    await act(async () => saving.reject(new Error('Disk write failed')));
    expect(result.current.draft).toBe('latest unsaved');
    expect(result.current.error).toContain('Disk write failed');
    expect(result.current.saving).toBe(false);
    await act(async () => result.current.refreshSource('local'));
    await act(async () => { await expect(result.current.flush()).rejects.toThrow('paused'); });
    expect(n.saveComposerDraft).toHaveBeenCalledTimes(1);
    expect(result.current.draft).toBe('latest unsaved');
  });

  it('reconciles a newer draft whose autosave conflicted with successful native Send', async () => {
    const n = native();
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('send this'));
    await act(async () => { await result.current.flush(); });
    // Native Send atomically cleared the persisted draft before its UI callback.
    n.states.set('a', snapshot('a', '', 99));
    act(() => result.current.setDraft('following message'));
    await autosave();
    expect(result.current.error).toContain('revision conflict');
    await act(async () => result.current.submitted('a', 'send this'));
    expect(result.current.error).toBeNull();
    expect(result.current.draft).toBe('following message');
    await autosave();
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a', 99, 'following message');
    expect(result.current.snapshot?.composer.revision).toBe(106);
  });

  it('exposes submission readback failure without clearing local text or guessing a revision', async () => {
    const n = native();
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'));
    await settle();
    act(() => result.current.setDraft('submitted draft'));
    await act(async () => { await result.current.flush(); });
    n.getComposer.mockRejectedValueOnce(new Error('Readback unavailable'));
    await act(async () => {
      await expect(result.current.submitted('a', 'submitted draft')).rejects.toThrow('Readback unavailable');
    });
    expect(result.current.draft).toBe('submitted draft');
    expect(result.current.snapshot?.composer.revision).toBe(17);
    await act(async () => { await expect(result.current.flush()).rejects.toThrow('paused'); });
    n.states.set('a', snapshot('a', '', 28));
    await act(async () => result.current.submitted('a', 'submitted draft'));
    expect(result.current.draft).toBe('');
    expect(result.current.snapshot?.composer.revision).toBe(28);
  });

  it('retains edits after load failure until explicit recovery; null chat cannot flush', async () => {
    const n = native();
    n.getComposer.mockRejectedValueOnce(new Error('Native unavailable'));
    const { result, rerender } = renderHook(({ id }: { id: string | null }) => useConversationComposer(n.gateway, id), { initialProps: { id: 'a' as string | null } });
    await settle();
    act(() => result.current.setDraft('offline draft'));
    await autosave();
    expect(result.current.error).toContain('Native unavailable');
    expect(result.current.snapshot).toBeNull();
    expect(n.saveComposerDraft).not.toHaveBeenCalled();
    await act(async () => { await expect(result.current.flush()).rejects.toThrow(); });
    await act(async () => result.current.refresh());
    expect(result.current.draft).toBe('offline draft');
    await act(async () => { await result.current.flush(); });
    rerender({ id: null });
    expect(result.current.snapshot).toBeNull();
    expect(result.current.draft).toBe('');
    await expect(result.current.flush()).rejects.toThrow('Select a conversation');
  });

  it('does not duplicate the initial load under StrictMode', async () => {
    const n = native();
    const wrapper = ({ children }: PropsWithChildren) => <StrictMode>{children}</StrictMode>;
    const { result } = renderHook(() => useConversationComposer(n.gateway, 'a'), { wrapper });
    await settle();
    expect(n.getComposer).toHaveBeenCalledTimes(1);
    expect(result.current.loading).toBe(false);
  });

  it('automatic metadata sync retains a conflicting edit and requires explicit recovery', async () => {
    const n=native();
    const {result}=renderHook(()=>useConversationComposer(n.gateway,'a'));
    await settle();
    act(()=>result.current.setDraft('my pending draft'));
    n.states.set('a',snapshot('a','other window draft',24));
    await act(async()=>result.current.sync());
    expect(result.current.draft).toBe('my pending draft');
    expect(result.current.error).toContain('changed while you were editing');
    await autosave();
    expect(n.saveComposerDraft).not.toHaveBeenCalled();
    await act(async()=>{await expect(result.current.flush()).rejects.toThrow('paused');});
    await act(async()=>result.current.refresh());
    expect(result.current.draft).toBe('my pending draft');
    await act(async()=>result.current.flush());
    expect(n.saveComposerDraft).toHaveBeenLastCalledWith('a',24,'my pending draft');
  });

  it('catalog refresh cannot adopt a newer revision and overwrite another window draft', async()=>{
    const n=native();
    const catalog=deferred<Awaited<ReturnType<typeof n.refreshComposerModels>>>();
    n.refreshComposerModels.mockImplementationOnce(()=>catalog.promise);
    const {result}=renderHook(()=>useConversationComposer(n.gateway,'a'));
    await settle();
    act(()=>result.current.setDraft('local edit'));
    await act(async()=>{
      const refresh=result.current.refreshSource('local');
      n.states.set('a',snapshot('a','other window',24));
      catalog.resolve({id:'local',label:'local',provider:'ollama',status:'ready',models:[]});
      await refresh;
    });
    await autosave();
    expect(n.saveComposerDraft).not.toHaveBeenCalled();
    expect(n.states.get('a')?.composer.draft).toBe('other window');
    expect(result.current.draft).toBe('local edit');
    expect(result.current.error).toContain('changed while you were editing');
  });

  it('rechecks cached chat availability when it becomes active again', async()=>{
    const n=native();
    const a=snapshot('a');
    a.connection={id:'membership:a',provider:'chatgpt',label:'A',model:'offered',baseUrl:'https://api.openai.com/v1',status:'connected',connectedAt:null,latencyMs:null};
    n.states.set('a',a);
    const {result,rerender}=renderHook(({id})=>useConversationComposer(n.gateway,id),{initialProps:{id:'a'}});
    await settle();expect(result.current.snapshot?.connection?.status).toBe('connected');
    rerender({id:'b'});await settle();
    n.states.set('a',{...a,connection:null,warning:'Reconnect account A'});
    rerender({id:'a'});await settle();
    expect(result.current.snapshot?.connection).toBeNull();
    expect(result.current.snapshot?.warning).toBe('Reconnect account A');
  });
});
