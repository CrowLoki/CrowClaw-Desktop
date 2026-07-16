import { Cable, CheckCircle2, Gauge, LockKeyhole, Server } from "lucide-react";
import type {
  ConnectionTestResult,
  DiscoveredEndpoint,
  ModelConnection,
  ModelEndpointDraft,
} from "../gateway/contracts";
import { ProviderForm } from "./ProviderForm";

type ConnectionsViewProps = {
  connection: ModelConnection;
  discovered: DiscoveredEndpoint[];
  onTest: (draft: ModelEndpointDraft) => Promise<ConnectionTestResult>;
  onConnect: (draft: ModelEndpointDraft) => Promise<void>;
};

export function ConnectionsView({ connection, discovered, onTest, onConnect }: ConnectionsViewProps) {
  const initialDraft: ModelEndpointDraft = {
    provider: connection.provider,
    label: connection.label,
    baseUrl: connection.baseUrl,
    model: connection.model,
  };

  return (
    <main className="section-view">
      <header className="section-heading">
        <div>
          <span className="eyebrow">Local runtime</span>
          <h1>Connections</h1>
          <p>Choose which local OpenAI-compatible model CrowClaw uses.</p>
        </div>
        <span className="section-stat section-stat--success"><CheckCircle2 size={18} /> Connected</span>
      </header>

      <section className="connection-overview" aria-labelledby="active-connection-title">
        <div className="connection-overview__icon"><Server size={24} /></div>
        <div className="connection-overview__identity">
          <span>Active connection</span>
          <h2 id="active-connection-title">{connection.label}</h2>
          <code>{connection.baseUrl}</code>
        </div>
        <dl>
          <div><dt>Model</dt><dd>{connection.model}</dd></div>
          <div><dt>Latency</dt><dd><Gauge size={14} /> {connection.latencyMs ?? "—"} ms</dd></div>
          <div><dt>Privacy</dt><dd><LockKeyhole size={14} /> Local endpoint</dd></div>
        </dl>
      </section>

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

