import { Check, LoaderCircle, LockKeyhole, MonitorCog, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import crowClawHero from "../assets/branding/crowclaw-hero.webp";
import type {
  ConnectionTestResult,
  DiscoveredEndpoint,
  ModelEndpointDraft,
} from "../gateway/contracts";
import { BrandMark } from "./BrandMark";
import { ProviderForm } from "./ProviderForm";

type OnboardingProps = {
  discoverEndpoints: () => Promise<DiscoveredEndpoint[]>;
  testConnection: (draft: ModelEndpointDraft) => Promise<ConnectionTestResult>;
  connect: (draft: ModelEndpointDraft) => Promise<void>;
};

const initialDraft: ModelEndpointDraft = {
  provider: "lm-studio",
  label: "LM Studio",
  baseUrl: "http://127.0.0.1:1234/v1",
  model: "local-model",
};

export function Onboarding({ discoverEndpoints, testConnection, connect }: OnboardingProps) {
  const [discovered, setDiscovered] = useState<DiscoveredEndpoint[]>([]);
  const [discovering, setDiscovering] = useState(true);

  useEffect(() => {
    let active = true;
    void discoverEndpoints()
      .then((endpoints) => {
        if (active) setDiscovered(endpoints);
      })
      .finally(() => {
        if (active) setDiscovering(false);
      });
    return () => {
      active = false;
    };
  }, [discoverEndpoints]);

  return (
    <main className="onboarding-shell">
      <div className="onboarding-glow" aria-hidden="true" />
      <section className="onboarding-intro" aria-labelledby="onboarding-title">
        <BrandMark />
        <div className="onboarding-intro__copy">
          <img
            className="onboarding-intro__artwork"
            src={crowClawHero}
            alt=""
            aria-hidden="true"
          />
          <span className="eyebrow"><Sparkles size={14} /> First run</span>
          <h1 id="onboarding-title">Your local agent,<br />ready to work.</h1>
          <p>
            Connect CrowClaw to a model running on this computer. Your conversations and approvals stay under your control.
          </p>
        </div>
        <ul className="assurance-list">
          <li><Check size={16} /> No paid service required</li>
          <li><LockKeyhole size={16} /> Actions wait for your approval</li>
          <li><MonitorCog size={16} /> Change providers whenever you want</li>
        </ul>
      </section>

      <section className="onboarding-panel" aria-labelledby="connection-title">
        <div className="onboarding-panel__brand"><BrandMark /></div>
        <div className="panel-heading">
          <div>
            <span className="step-label">Step 1 of 1</span>
            <h2 id="connection-title">Connect a local model</h2>
            <p>Choose a detected server or enter any OpenAI-compatible endpoint.</p>
          </div>
          {discovering && <span className="discovering"><LoaderCircle className="spin" size={16} /> Looking locally</span>}
        </div>
        <ProviderForm
          initialDraft={discovered[0] ?? initialDraft}
          discovered={discovered}
          submitLabel="Connect and open CrowClaw"
          onTest={testConnection}
          onSubmit={connect}
        />
        <p className="onboarding-footnote">CrowClaw connects only to the endpoint shown above. You can review permissions in Settings.</p>
      </section>
    </main>
  );
}

