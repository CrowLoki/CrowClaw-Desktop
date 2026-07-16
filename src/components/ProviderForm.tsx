import { CheckCircle2, LoaderCircle, Radio, Server, WifiOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type {
  ConnectionTestResult,
  DiscoveredEndpoint,
  ModelEndpointDraft,
  ProviderKind,
} from "../gateway/contracts";

type ProviderFormProps = {
  initialDraft: ModelEndpointDraft;
  discovered: DiscoveredEndpoint[];
  submitLabel: string;
  onTest: (draft: ModelEndpointDraft) => Promise<ConnectionTestResult>;
  onSubmit: (draft: ModelEndpointDraft) => Promise<void>;
};

const providers: Array<{
  id: ProviderKind;
  name: string;
  description: string;
  baseUrl: string;
  model: string;
}> = [
  {
    id: "lm-studio",
    name: "LM Studio",
    description: "Desktop local model server",
    baseUrl: "http://127.0.0.1:1234/v1",
    model: "local-model",
  },
  {
    id: "ollama",
    name: "Ollama",
    description: "Local model runtime",
    baseUrl: "http://127.0.0.1:11434/v1",
    model: "qwen3.5:9b",
  },
  {
    id: "llama-cpp",
    name: "llama.cpp",
    description: "Local OpenAI-compatible server",
    baseUrl: "http://127.0.0.1:8080/v1",
    model: "local-model",
  },
  {
    id: "custom",
    name: "Custom",
    description: "Another compatible endpoint",
    baseUrl: "http://127.0.0.1:8000/v1",
    model: "local-model",
  },
];

export function ProviderForm({
  initialDraft,
  discovered,
  submitLabel,
  onTest,
  onSubmit,
}: ProviderFormProps) {
  const [draft, setDraft] = useState<ModelEndpointDraft>(initialDraft);
  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(null);
  const [testing, setTesting] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const userModified = useRef(false);

  useEffect(() => {
    if (discovered.length === 0 || userModified.current) return;
    setDraft(discovered[0]);
    setTestResult(null);
  }, [discovered]);

  function updateDraft(patch: Partial<ModelEndpointDraft>) {
    userModified.current = true;
    setDraft((current) => ({ ...current, ...patch }));
    setTestResult(null);
    setError(null);
  }

  function selectProvider(provider: ProviderKind) {
    const preset = providers.find(({ id }) => id === provider);
    if (!preset) return;
    const detected = discovered.find((candidate) => candidate.provider === provider);
    updateDraft({
      provider,
      label: detected?.label ?? preset.name,
      baseUrl: detected?.baseUrl ?? preset.baseUrl,
      model: detected?.model ?? preset.model,
      apiKey: provider === "custom" ? draft.apiKey : undefined,
    });
  }

  async function testEndpoint() {
    setTesting(true);
    setError(null);
    try {
      setTestResult(await onTest(draft));
    } catch (cause) {
      setTestResult(null);
      setError(cause instanceof Error ? cause.message : "The endpoint could not be tested.");
    } finally {
      setTesting(false);
    }
  }

  async function submit() {
    if (!testResult?.ok) return;
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(draft);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The connection could not be saved.");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="provider-form">
      <fieldset className="provider-picker">
        <legend className="sr-only">Choose a local model provider</legend>
        {providers.map((provider) => {
          const detected = discovered.some((candidate) => candidate.provider === provider.id);
          const selected = draft.provider === provider.id;
          return (
            <label className={selected ? "provider-card provider-card--selected" : "provider-card"} key={provider.id}>
              <input
                type="radio"
                name="provider"
                value={provider.id}
                checked={selected}
                onChange={() => selectProvider(provider.id)}
              />
              <span className="provider-card__icon" aria-hidden="true">
                {provider.id === "custom" ? <Server size={19} /> : <Radio size={19} />}
              </span>
              <span className="provider-card__copy">
                <strong>{provider.name}</strong>
                <small>{provider.description}</small>
              </span>
              {detected && <span className="detected-badge">Detected</span>}
            </label>
          );
        })}
      </fieldset>

      <div className="form-grid">
        <label className="field field--wide">
          <span>Endpoint URL</span>
          <input
            type="url"
            value={draft.baseUrl}
            onChange={(event) => updateDraft({ baseUrl: event.currentTarget.value })}
            placeholder="http://127.0.0.1:1234/v1"
            autoComplete="url"
          />
        </label>
        <label className="field">
          <span>Connection name</span>
          <input
            value={draft.label}
            onChange={(event) => updateDraft({ label: event.currentTarget.value })}
            placeholder="My local model"
            autoComplete="off"
          />
        </label>
        <label className="field">
          <span>Model name</span>
          <input
            value={draft.model}
            onChange={(event) => updateDraft({ model: event.currentTarget.value })}
            placeholder="local-model"
            autoComplete="off"
          />
        </label>
        {draft.provider === "custom" && (
          <label className="field field--wide">
            <span>API key <small>Optional; kept only for this app session</small></span>
            <input
              type="password"
              value={draft.apiKey ?? ""}
              onChange={(event) => updateDraft({ apiKey: event.currentTarget.value })}
              placeholder="Leave blank for a local endpoint"
              autoComplete="new-password"
            />
          </label>
        )}
      </div>

      <div className="connection-result" aria-live="polite">
        {testing && (
          <span className="connection-result__neutral"><LoaderCircle className="spin" size={16} /> Testing local endpoint…</span>
        )}
        {!testing && testResult?.ok && (
          <span className="connection-result__success">
            <CheckCircle2 size={16} /> {testResult.detail} {testResult.latencyMs} ms
          </span>
        )}
        {!testing && testResult && !testResult.ok && (
          <span className="connection-result__error"><WifiOff size={16} /> {testResult.detail}</span>
        )}
        {error && <span className="connection-result__error"><WifiOff size={16} /> {error}</span>}
      </div>

      <div className="form-actions">
        <button className="button button--secondary" type="button" onClick={() => void testEndpoint()} disabled={testing || submitting}>
          {testing ? <LoaderCircle className="spin" size={17} /> : <Radio size={17} />}
          Test connection
        </button>
        <button className="button button--primary" type="button" onClick={() => void submit()} disabled={!testResult?.ok || submitting}>
          {submitting ? <LoaderCircle className="spin" size={17} /> : <CheckCircle2 size={17} />}
          {submitLabel}
        </button>
      </div>
    </div>
  );
}

