import { useCallback, useEffect, useState, type FormEvent } from "react";
import type { CrowClawGateway, MemorySearchMode, MemorySearchResult, MemorySettings, NativeMemoryStatus } from "../gateway/contracts";
import { SemanticMemorySettings } from "./SemanticMemorySettings";

type Props = { gateway: CrowClawGateway };

const sourceLabels: Record<string, string> = { conversation_message: "Conversation", approved_action: "Approved action", user_note: "Your note", approved_file: "Remembered file", legacy_crowquant: "Earlier CrowQuant note" };

export function NativeMemoryPanel({ gateway }: Props) {
  const [status, setStatus] = useState<NativeMemoryStatus | null>(null);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<MemorySearchMode>("hybrid");
  const [source, setSource] = useState("");
  const [result, setResult] = useState<MemorySearchResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const refresh = useCallback(async () => setStatus(await gateway.memoryStatus()), [gateway]);

  useEffect(() => {
    let active = true;
    let loading = false;
    const update = async () => {
      if (loading) return;
      loading = true;
      try { const value = await gateway.memoryStatus(); if (active) setStatus(value); }
      catch (cause) { if (active) setError(String(cause)); }
      finally { loading = false; }
    };
    void update();
    const timer = setInterval(() => void update(), 2000);
    return () => { active = false; clearInterval(timer); };
  }, [gateway]);

  async function perform(work: () => Promise<void>) {
    setBusy(true); setError(null); setNotice(null);
    try { await work(); await refresh(); } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); } finally { setBusy(false); }
  }

  async function configure(next: MemorySettings) {
    await perform(async () => { await gateway.configureMemory(next); const report = await gateway.syncMemory(); setNotice(report.warnings.join(" ") || "Local memory settings saved."); setResult(null); });
  }

  function search(event: FormEvent) {
    event.preventDefault();
    void perform(async () => { const report = await gateway.syncMemory(); setResult(await gateway.searchMemory({ query, limit: 10, sourceKind: source || null, mode })); if (report.pending) setNotice(`${report.pending} sources are still waiting to be indexed.`); });
  }

  return <section className="crowquant-panel" aria-labelledby="native-memory-title">
    <header className="crowquant-panel__heading"><div><span className="eyebrow">Local recall</span><h2 id="native-memory-title">Search retained context</h2><p>Find past conversations and remembered notes on this computer. Results stay here unless you approve an agent memory read.</p></div>{status && <span>{status.activeSources} sources · {status.pending} pending</span>}</header>
    {status?.settings.indexConversations === null && <div role="status"><p>Build a local search index from conversations already stored by CrowClaw?</p><button type="button" className="button button--primary" disabled={busy} onClick={() => void configure({ ...status.settings, indexConversations: true })}>Index my CrowClaw conversations</button><button type="button" className="button button--secondary" disabled={busy} onClick={() => void configure({ ...status.settings, indexConversations: false })}>Keep conversation indexing off</button></div>}
    {status && <div className="permission-rows">
      <label className="setting-row"><span>Index CrowClaw conversations</span><input type="checkbox" checked={status.settings.indexConversations === true} disabled={busy} onChange={event => void configure({ ...status.settings, indexConversations: event.currentTarget.checked })} /></label>
      <label className="setting-row"><span>Index successful approved-action summaries</span><input type="checkbox" checked={status.settings.indexActions} disabled={busy} onChange={event => void configure({ ...status.settings, indexActions: event.currentTarget.checked })} /></label>
    </div>}
    {status && <SemanticMemorySettings settings={status.settings} status={status.semantic} busy={busy} onConfigure={configure} onIndex={() => perform(async () => { const report = await gateway.syncSemanticMemory(); setNotice(`Stored ${report.indexed} semantic vectors; ${report.pending} pending. ${report.warnings.join(" ")}`); })} />}
    <form className="crowquant-form" onSubmit={search}>
      <label htmlFor="native-memory-query">Search previous context</label><input id="native-memory-query" value={query} onChange={e => setQuery(e.currentTarget.value)} placeholder="What did we decide about the telescope?" />
      <div className="view-toolbar"><label>Search method <select value={mode} onChange={e => setMode(e.currentTarget.value as MemorySearchMode)}><option value="hybrid">Combined search</option><option value="full_text">Keywords</option><option value="lexical">CrowQuant lexical</option><option value="semantic">Semantic, with offline fallback</option></select></label><label>Source <select value={source} onChange={e => setSource(e.currentTarget.value)}><option value="">All indexed sources</option>{Object.entries(sourceLabels).map(([key,label]) => <option key={key} value={key}>{label}</option>)}</select></label><button type="submit" className="button button--secondary" disabled={busy || !query.trim()}>Search context</button></div>
    </form>
    <div className="view-toolbar"><button type="button" className="button button--secondary" disabled={busy} onClick={() => void perform(async () => { const report = await gateway.rebuildMemory(); setResult(null); setNotice(`Rebuilt ${report.indexed} sources; ${report.pending} pending. Original conversations and notes were kept.`); })}>Rebuild search index</button><button type="button" className="button button--secondary" disabled={busy} onClick={() => void perform(async () => {
      const result = await gateway.exportMemory(); setNotice(result.saved ? `Memory exported to ${result.fileName}.` : "Export cancelled. No file was written.");
    })}>Export memory</button><button type="button" className="button button--secondary" disabled={busy} onClick={() => void perform(async () => { const report = await gateway.syncMemory(); setNotice(`Indexed ${report.indexed}; ${report.pending} pending. ${report.warnings.join(" ")}`); })}>Update index</button></div>
    {error && <p role="alert" className="inline-error">{error}</p>}{notice && <p role="status">{notice}</p>}
    {status?.warnings.map(w => <p key={w} role="status">{w}</p>)}{result?.warnings.map(w => <p key={w} role="status">{w}</p>)}
    {result && !result.hits.length && <p>No matching indexed context. Check source filters and indexing settings.</p>}
    <div className="memory-grid">{result?.hits.map(hit => <article className="memory-card" key={hit.chunkId}><div className="memory-card__source">{sourceLabels[hit.sourceKind] || hit.sourceKind} · {hit.authorship}</div><h3>{hit.title}</h3><p>{hit.text}</p><div className="tag-row">{hit.channels.map(channel => <span key={channel.channel}>{channel.channel === "full_text" ? "Keyword match" : channel.channel === "exact_text" ? "Exact text match" : channel.channel === "semantic" ? "Semantic match" : "Lexical match"}</span>)}</div><time dateTime={new Date(hit.createdAtMs).toISOString()}>{new Date(hit.createdAtMs).toLocaleString()}</time><button type="button" className="button button--danger-quiet" disabled={busy} onClick={() => void perform(async () => { await gateway.withdrawMemory(hit.sourceId); setResult(current => current ? { ...current, hits: current.hits.filter(h => h.sourceId !== hit.sourceId) } : current); setNotice("Removed from memory search. The original conversation or note is retained."); })}>Forget from search</button></article>)}</div>
  </section>;
}
