import { useCallback, useEffect, useRef, useState } from 'react';
import type { CrowClawGateway, ModelConnection } from '../gateway/contracts';
import type { FreeCatalog } from '../gateway/openRouterContracts';
import './OpenRouterConnection.css';

type Props = {
  gateway: Pick<CrowClawGateway, 'openRouterCatalog' | 'connectOpenRouter' | 'disconnectOpenRouter'>;
  connection: ModelConnection | null;
  hasConversation: boolean;
  onConnected: (connection: ModelConnection, useCurrentChat: boolean) => Promise<void>;
  onDisconnected: (profileId: string) => void;
  browserPreview?: boolean;
};

export function OpenRouterConnection({ gateway, connection, hasConversation, onConnected, onDisconnected, browserPreview = false }: Props) {
  const profileId = connection?.provider === 'openrouter' && connection.status === 'connected' ? connection.id : undefined;
  const [catalog, setCatalog] = useState<FreeCatalog | null>(null);
  const [loading, setLoading] = useState(true);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [modelId, setModelId] = useState(connection?.provider === 'openrouter' ? connection.model : '');
  const [label, setLabel] = useState('OpenRouter FREE');
  const [apiKey, setApiKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const generation = useRef(0);
  const operation = useRef(false);

  const refresh = useCallback(async () => {
    const request = ++generation.current;
    setLoading(true);
    setCatalogError(null);
    try {
      const next = await gateway.openRouterCatalog(profileId);
      if (request === generation.current) setCatalog(next);
    } catch {
      if (request === generation.current) setCatalogError(browserPreview
        ? 'Live catalog is unavailable in browser preview. Open the installed desktop app to browse OpenRouter.'
        : 'Could not refresh the free catalog. Check your connection and retry.');
    } finally {
      if (request === generation.current) setLoading(false);
    }
  }, [gateway, profileId, browserPreview]);

  useEffect(() => {
    void refresh();
    return () => { ++generation.current; };
  }, [refresh]);

  const selected = catalog?.models.find(model => model.id === modelId);
  const staleSelection = !!catalog && !!modelId && !selected;
  const matches = catalog?.models.filter(model => `${model.name} ${model.id}`.toLowerCase().includes(query.toLowerCase())) ?? [];
  const canSave = !!selected && !!apiKey.trim() && !!label.trim() && !loading && !catalogError && !busy;

  async function save(useCurrentChat: boolean) {
    if (!canSave || operation.current) return;
    operation.current = true;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      let saved: ModelConnection;
      try {
        saved = await gateway.connectOpenRouter({ label: label.trim(), apiKey: apiKey.trim(), model: modelId });
      } catch {
        // Native errors may contain provider response bodies. Never echo key-bearing data.
        setError('OpenRouter connection could not be saved. Check your API key, refresh the free catalog and retry.');
        return;
      }
      setApiKey('');
      try {
        await onConnected(saved, useCurrentChat);
        setNotice(useCurrentChat ? 'Connection saved and selected for this chat’s next message.' : 'Connection saved as the default for new chats. This chat’s model is unchanged.');
      } catch {
        setError('Connection saved as the default, but selecting it for this chat failed. Your key was saved; choose the profile and model in the chat’s model controls to retry.');
      }
    } finally {
      operation.current = false;
      setBusy(false);
    }
  }

  async function disconnect() {
    if (!profileId || operation.current) return;
    operation.current = true;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await gateway.disconnectOpenRouter(profileId);
      setApiKey('');
      onDisconnected(profileId);
      setNotice('This OpenRouter key was removed from this device. Chats are retained.');
    } catch {
      setError('Could not disconnect this OpenRouter connection. Retry to remove its saved key.');
    } finally {
      operation.current = false;
      setBusy(false);
    }
  }

  return <section className="settings-panel openrouter-panel" aria-labelledby="openrouter-title">
    <h2 id="openrouter-title">OpenRouter FREE</h2>
    <p>Browse free models without a login, API key or payment. Real requests require your own OpenRouter API key. Free-only routing; no paid fallback.</p>
    {browserPreview && <p>Browser preview does not load a live catalog or save keys. Use the installed desktop app.</p>}
    <div className="openrouter-actions">
      <button type="button" className="button button--secondary" disabled={loading || busy} onClick={() => void refresh()}>{catalogError ? 'Retry catalog' : 'Refresh free catalog'}</button>
      {catalog && <span>{catalog.models.length} free models · Refreshed <time dateTime={new Date(catalog.fetchedAtMs).toISOString()}>{new Date(catalog.fetchedAtMs).toLocaleString()}</time></span>}
    </div>
    {loading && <p role="status">Loading free catalog…</p>}
    {catalogError && <p role="alert">{catalogError}{catalog ? ' The displayed catalog is stale; refresh before saving.' : ''}</p>}
    {!loading && catalog?.models.length === 0 && <p role="status">No free models are currently available. Refresh to check again.</p>}
    <label htmlFor="openrouter-search">Search free models</label>
    <input id="openrouter-search" type="search" value={query} onChange={event => setQuery(event.target.value)} />
    <label htmlFor="openrouter-model">Free model</label>
    <select id="openrouter-model" value={modelId} disabled={loading || busy || !catalog?.models.length || !!catalogError} onChange={event => { setModelId(event.target.value); setNotice(null); }}>
      <option value="">Choose a free model</option>
      {staleSelection && <option value={modelId} disabled>{modelId} — no longer available</option>}
      {selected && !matches.some(model => model.id === selected.id) && <option value={selected.id}>{selected.name} ({selected.id}) — selected</option>}
      {matches.map(model => <option key={model.id} value={model.id}>{model.name} ({model.id})</option>)}
    </select>
    {!!catalog?.models.length && !matches.length && <p role="status">No free models match your search.</p>}
    {staleSelection && <p role="alert">The selected model is no longer in the free catalog. Choose another model before saving.</p>}
    {selected && <dl className="openrouter-metadata">
      <div><dt>Model ID</dt><dd>{selected.id}</dd></div>
      <div><dt>Context</dt><dd>{selected.contextLength > 0 ? `${selected.contextLength.toLocaleString()} tokens` : 'Not reported'}</dd></div>
      <div><dt>Input modalities</dt><dd>{selected.inputModalities.join(', ') || 'Not reported'}</dd></div>
      <div><dt>Tools</dt><dd>{selected.supportedParameters.includes('tools') ? 'Listed as supported' : 'Not listed by the catalog'}</dd></div>
      <div><dt>Supported parameters</dt><dd>{selected.supportedParameters.join(', ') || 'Not reported'}</dd></div>
      <div><dt>Reasoning</dt><dd>{selected.reasoningEfforts.length ? selected.reasoningEfforts.join(', ') : 'Provider default; no effort levels reported'}</dd></div>
    </dl>}
    <label htmlFor="openrouter-label">Connection name</label>
    <input id="openrouter-label" value={label} disabled={busy} onChange={event => setLabel(event.target.value)} />
    <label htmlFor="openrouter-key">OpenRouter API key</label>
    <p id="openrouter-key-hint">Saved by the native app using Windows DPAPI. The key is cleared from this form after saving.</p>
    <input id="openrouter-key" type="password" autoComplete="off" spellCheck={false} aria-describedby="openrouter-key-hint" value={apiKey} disabled={busy || browserPreview} onChange={event => setApiKey(event.target.value)} />
    <div className="openrouter-actions">
      <button type="button" className="button button--secondary" disabled={!canSave} onClick={() => void save(false)}>Save for new chats</button>
      {hasConversation && <button type="button" className="button button--primary" disabled={!canSave} onClick={() => void save(true)}>Save and use in this chat</button>}
    </div>
    {profileId && <div>
      <p>Disconnect removes only the saved key for {connection?.label}. It keeps your chats and other provider credentials.</p>
      <button type="button" className="button button--secondary" disabled={busy} onClick={() => void disconnect()}>Disconnect this OpenRouter connection</button>
    </div>}
    {busy && <p role="status">Updating OpenRouter connection…</p>}
    {error && <p role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
  </section>;
}
