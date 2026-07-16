import { Check, Database, HardDrive, LoaderCircle, Moon, Save, ShieldCheck, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import type { AppSettings, PermissionMode } from "../gateway/contracts";

type SettingsViewProps = {
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<void>;
};

const permissionOptions: Array<{ value: PermissionMode; label: string }> = [
  { value: "ask", label: "Ask every time" },
  { value: "allow-session", label: "Allow for this session" },
  { value: "deny", label: "Always deny" },
];

export function SettingsView({ settings, onSave }: SettingsViewProps) {
  const [draft, setDraft] = useState(settings);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setDraft(settings), [settings]);

  function setPermission(key: keyof AppSettings["permissions"], value: PermissionMode) {
    setDraft((current) => ({
      ...current,
      permissions: { ...current.permissions, [key]: value },
    }));
    setSaved(false);
  }

  function setBoolean(key: "launchAtLogin" | "keepRunningOnClose" | "retainConversations", value: boolean) {
    setDraft((current) => ({ ...current, [key]: value }));
    setSaved(false);
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      await onSave(draft);
      setSaved(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Settings could not be saved.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <main className="section-view">
      <header className="section-heading">
        <div>
          <span className="eyebrow">CrowClaw preferences</span>
          <h1>Settings</h1>
          <p>Control local permissions, startup behavior, and conversation retention.</p>
        </div>
        {saved && <span className="section-stat section-stat--success"><Check size={18} /> Saved</span>}
      </header>

      <section className="settings-panel" aria-labelledby="permissions-title">
        <div className="settings-panel__heading">
          <div><span className="settings-icon"><ShieldCheck size={18} /></span><div><h2 id="permissions-title">Action permissions</h2><p>These defaults never override an action-specific denial.</p></div></div>
        </div>
        <div className="permission-rows">
          <label className="setting-row"><span><strong>Read local files</strong><small>Inspect files you deliberately select</small></span><select value={draft.permissions.readFiles} onChange={(event) => setPermission("readFiles", event.currentTarget.value as PermissionMode)}>{permissionOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</select></label>
          <label className="setting-row"><span><strong>Create or change files</strong><small>Write content inside approved locations</small></span><select value={draft.permissions.writeFiles} onChange={(event) => setPermission("writeFiles", event.currentTarget.value as PermissionMode)}>{permissionOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</select></label>
          <label className="setting-row"><span><strong>Run local commands</strong><small>Execute a command shown in the approval request</small></span><select value={draft.permissions.runCommands} onChange={(event) => setPermission("runCommands", event.currentTarget.value as PermissionMode)}>{permissionOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</select></label>
        </div>
      </section>

      <section className="settings-panel" aria-labelledby="desktop-title">
        <div className="settings-panel__heading">
          <div><span className="settings-icon"><HardDrive size={18} /></span><div><h2 id="desktop-title">Desktop behavior</h2><p>Choose how CrowClaw behaves on this Windows account.</p></div></div>
        </div>
        <div className="toggle-rows">
          <label className="toggle-row"><span><Sparkles size={17} /><span><strong>Launch when I sign in</strong><small>Start CrowClaw with Windows</small></span></span><input type="checkbox" checked={draft.launchAtLogin} onChange={(event) => setBoolean("launchAtLogin", event.currentTarget.checked)} /></label>
          <label className="toggle-row"><span><Moon size={17} /><span><strong>Keep working when the window closes</strong><small>Use Quit to stop CrowClaw completely</small></span></span><input type="checkbox" checked={draft.keepRunningOnClose} onChange={(event) => setBoolean("keepRunningOnClose", event.currentTarget.checked)} /></label>
          <label className="toggle-row"><span><Database size={17} /><span><strong>Retain conversation history</strong><small>Continue previous conversations after reopening</small></span></span><input type="checkbox" checked={draft.retainConversations} onChange={(event) => setBoolean("retainConversations", event.currentTarget.checked)} /></label>
        </div>
      </section>

      {error && <div className="inline-error" role="alert">{error}</div>}
      <div className="settings-save-row">
        <p>Changes are stored locally for this CrowClaw profile.</p>
        <button className="button button--primary" type="button" onClick={() => void save()} disabled={saving}>
          {saving ? <LoaderCircle className="spin" size={17} /> : <Save size={17} />}
          Save settings
        </button>
      </div>
    </main>
  );
}

