import { Cable, CheckCircle2, Gauge, LockKeyhole, Server } from "lucide-react";
import type {
  ConnectionTestResult,
  DiscoveredEndpoint,
  ModelConnection,
  ModelEndpointDraft,
  CrowClawGateway,
} from "../gateway/contracts";
import { ProviderForm } from "./ProviderForm";
import { OpenRouterConnection } from './OpenRouterConnection';

type ConnectionsViewProps = {
  gateway: CrowClawGateway;
  hasConversation: boolean;
  onOpenRouterConnected: (connection: ModelConnection, useCurrentChat: boolean) => Promise<void>;
  onOpenRouterDisconnected: (profileId: string) => void;
  browserPreview: boolean;
  connection: ModelConnection;
  discovered: DiscoveredEndpoint[];
  onTest: (draft: ModelEndpointDraft) => Promise<ConnectionTestResult>;
  onConnect: (draft: ModelEndpointDraft) => Promise<void>;
};

export function ConnectionsView({ connection, discovered, onTest, onConnect, gateway, hasConversation, onOpenRouterConnected, onOpenRouterDisconnected, browserPreview }: ConnectionsViewProps) {
  const initialDraft: ModelEndpointDraft = connection.provider === "chatgpt" || connection.provider === "openrouter" ? {
    provider: "lm-studio", label: "LM Studio", baseUrl: "http://127.0.0.1:1234/v1", model: "local-model",
  } : {
    provider: connection.provider,
    label: connection.label,
    baseUrl: connection.baseUrl,
    model: connection.model,
  };

  return (
    <main className="section-view">
      <header className="section-heading">
        <div>
          <span className="eyebrow">Model connections</span>
          <h1>Connections</h1>
          <p>Choose a local model or connect to OpenRouter’s free catalog.</p>
        </div>
        <span className={`section-stat ${connection.status === "connected" ? "section-stat--success" : ""}`}><CheckCircle2 size={18} /> {connection.status === "connected" ? "Connected" : "Disconnected"}</span>
      </header>

      <section className="connection-overview" aria-labelledby="active-connection-title">
        <div className="connection-overview__icon"><Server size={24} /></div>
        <div className="connection-overview__identity">
          <span>Default connection for new chats</span>
          <h2 id="active-connection-title">{connection.label}</h2>
          {connection.provider === "chatgpt" ? <span>ChatGPT account · Manage in Settings</span> : <code>{connection.baseUrl}</code>}
        </div>
        <dl>
          <div><dt>Model</dt><dd>{connection.model}</dd></div>
          <div><dt>Latency</dt><dd><Gauge size={14} /> {connection.latencyMs ?? "—"} ms</dd></div>
          <div><dt>Provider</dt><dd><LockKeyhole size={14} /> {connection.provider === "chatgpt" ? "ChatGPT" : connection.provider === "openrouter" ? "OpenRouter FREE" : "Local / custom endpoint"}</dd></div>
        </dl>
      </section>

      <OpenRouterConnection gateway={gateway} connection={connection} hasConversation={hasConversation} onConnected={onOpenRouterConnected} onDisconnected={onOpenRouterDisconnected} browserPreview={browserPreview} />

      <section className="settings-panel" aria-labelledby="change-connection-title">
        <div className="settings-panel__heading">
          <div><span className="settings-icon"><Cable size={18} /></span><div><h2 id="change-connection-title">Change model connection</h2><p>Test a new endpoint before making it active.</p></div></div>
        </div>
        <ProviderForm
          initialDraft={initialDraft}
          discovered={discovered}
          submitLabel="Use this connection"
          onTest={onTest}
          onSubmit={onConnect}
        />
      </section>
    </main>
  );
}
