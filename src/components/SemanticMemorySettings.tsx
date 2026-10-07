import { useEffect, useState } from "react";
import type { EmbeddingProfile, MemorySettings, SemanticStatus } from "../gateway/contracts";

type Props = {
  settings: MemorySettings;
  status?: SemanticStatus;
  busy: boolean;
  onConfigure: (settings: MemorySettings) => Promise<void>;
  onIndex: () => Promise<void>;
};

const defaultProfile: EmbeddingProfile = { provider: "ollama", baseUrl: "http://127.0.0.1:11434", model: "", dimensions: 768 };

export function SemanticMemorySettings({ settings, status, busy, onConfigure, onIndex }: Props) {
  const [enabled, setEnabled] = useState(Boolean(settings.embedding));
  const [draft, setDraft] = useState<EmbeddingProfile>(settings.embedding ?? defaultProfile);
  const profileKey = JSON.stringify(settings.embedding ?? null);
  useEffect(() => {
    const profile = JSON.parse(profileKey) as EmbeddingProfile | null;
    setEnabled(Boolean(profile));
    setDraft(profile ?? defaultProfile);
  }, [profileKey]);

  return <section className="settings-panel" aria-labelledby="semantic-memory-title">
    <header className="settings-panel__heading"><div><h3 id="semantic-memory-title">Optional semantic recall</h3><p>Find related meaning with a local embedding model. Keyword and CrowQuant search remain available when it is stopped.</p></div></header>
    <form className="crowquant-form" onSubmit={event => { event.preventDefault(); void onConfigure({ ...settings, embedding: enabled ? draft : null }); }}>
      <label className="setting-row"><span>Enable a local embedding profile</span><input type="checkbox" checked={enabled} onChange={event => setEnabled(event.currentTarget.checked)} /></label>
      {enabled && <>
        <p>Text from the indexed sources you enable and semantic search queries will be sent to this model server on your computer. CrowClaw does not download models or reuse your chat credentials.</p>
        <label>Embedding protocol<select value={draft.provider} disabled={busy} onChange={event => { const provider = event.currentTarget.value as EmbeddingProfile["provider"]; setDraft(current => ({ ...current, provider })); }}><option value="ollama">Ollama</option><option value="openai">OpenAI-compatible local server</option></select></label>
        <label>Local embedding endpoint<input type="url" value={draft.baseUrl} required disabled={busy} onChange={event => { const baseUrl = event.currentTarget.value; setDraft(current => ({ ...current, baseUrl })); }} /></label>
        <label>Embedding model identifier<input value={draft.model} required disabled={busy} placeholder="Exact model name installed in your server" onChange={event => { const model = event.currentTarget.value; setDraft(current => ({ ...current, model })); }} /></label>
        <label>Model output dimensions<input type="number" min={1} max={4096} step={1} value={draft.dimensions} required disabled={busy} onChange={event => { const dimensions = Number(event.currentTarget.value); setDraft(current => ({ ...current, dimensions })); }} /></label>
      </>}
      <div className="view-toolbar"><button type="submit" className="button button--secondary" disabled={busy}>Save semantic profile</button>{settings.embedding && <button type="button" className="button button--danger-quiet" onClick={() => void onConfigure({ ...settings, embedding: null })}>Disable semantic retrieval now</button>}<button type="button" className="button button--secondary" disabled={busy || !settings.embedding} onClick={() => void onIndex()}>Update semantic index</button></div>
    </form>
    {status && <p role="status">{status.state === "disabled" ? "Semantic retrieval is off." : `${status.vectors} semantic vectors stored · ${status.pending} pending. ${status.detail ?? "Server availability is checked when a request runs."}`}</p>}
  </section>;
}
