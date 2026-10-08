import { useEffect, useId, useRef } from "react";
import type { ModelConnection } from "../gateway/contracts";
import { membershipModels, type MembershipController } from "./useMembershipAccounts";

export function MembershipAccounts({ membership, connection }: { membership: MembershipController; connection: ModelConnection | null }) {
  const id = useId();
  const errorRef = useRef<HTMLDivElement>(null);
  const { account, selectedId, choice, label } = membership;
  const models = membershipModels(account);
  const model = models.find((item) => item.slug === choice.model);
  const effortUnavailable = choice.reasoningEffort !== null && !model?.reasoningEfforts.includes(choice.reasoningEffort);
  const busy = membership.pending !== null || membership.cancelling;
  const active = connection?.provider === "chatgpt" && connection.id === `membership:${selectedId}` && connection.status === "connected";
  const error = membership.error ?? membership.catalogError;

  useEffect(() => { if (error) errorRef.current?.focus(); }, [error]);

  return (
    <section className="settings-panel membership-panel" aria-labelledby={`${id}-title`}>
      <h2 id={`${id}-title`}>Use your ChatGPT plan</h2>
      <p>Sign in to use your ChatGPT plan for eligible AI requests in CrowClaw. Local models remain available in Connections.</p>
      {membership.loading && <p role="status">Loading saved accounts locally…</p>}
      {!membership.loaded && !membership.loading && <button className="button button--secondary" type="button" onClick={() => void membership.retrySnapshot()}>Retry saved accounts</button>}
      <label className="field" htmlFor={`${id}-account`}>Saved ChatGPT account</label>
      <select id={`${id}-account`} name="membershipAccount" value={selectedId} disabled={membership.loading || !membership.loaded || busy} onChange={(event) => membership.select(event.currentTarget.value)}>
        <option value="">Add a separate registration</option>
        {membership.accounts.map((item) => <option key={item.id} value={item.id}>{item.label} · {item.identity.email ?? "Email unavailable"} · {item.hasCredentials ? "Signed in" : "Signed out"}</option>)}
      </select>
      <p className="membership-help">Each registration is saved separately, even when the email address is the same.</p>
      <form onSubmit={(event) => { event.preventDefault(); void membership.signIn(); }} noValidate>
        <label className="field" htmlFor={`${id}-label`}>Registration label (required)</label>
        <input id={`${id}-label`} name="membershipLabel" value={label} required autoComplete="off" disabled={membership.loading || !membership.loaded}
          aria-describedby={`${id}-label-help`} onChange={(event) => membership.setLabel(event.currentTarget.value)} />
        <p id={`${id}-label-help`} className="membership-help">{account ? "Continue to reconnect this saved registration and save its label." : "Choose a label to distinguish this registration, such as Personal or Work."}</p>
        <div className="membership-actions">
          <button className="button chatgpt-sign-in" type="submit" disabled={membership.loading || !membership.loaded || busy || membership.refreshing}>Continue with ChatGPT</button>
          {membership.pending === "sign-in" && <button className="button button--secondary" type="button" disabled={membership.cancelling} onClick={() => void membership.cancelSignIn()}>{membership.cancelling ? "Cancelling sign-in…" : "Cancel sign-in"}</button>}
        </div>
      </form>
      {account && (
        <div className="membership-account">
          <p>{account.label} · {account.identity.email ?? "Email unavailable"} · {account.hasCredentials ? "Signed in" : "Signed out"}</p>
          {active && <p className="membership-plan">Using ChatGPT plan · {connection.model}</p>}
          {account.hasCredentials && (
            <>
              <div className="membership-actions">
                <button type="button" className="button button--secondary" disabled={busy || membership.refreshing} onClick={membership.refresh}>Refresh models</button>
                <button type="button" className="button button--danger-quiet" disabled={busy || membership.refreshing} onClick={() => void membership.signOut()}>Sign out of this account</button>
              </div>
              {membership.refreshing && <p role="status">Refreshing models for {account.label}…</p>}
              {!membership.refreshing && !membership.catalogError && models.length === 0 && <p>No models were returned for this account. Refresh models or reconnect to try again.</p>}
              <form onSubmit={(event) => { event.preventDefault(); void membership.useModel(); }} noValidate>
                <fieldset disabled={membership.refreshing || !!membership.catalogError}>
                  <legend>Model for {account.label}</legend>
                  <label className="field" htmlFor={`${id}-model`}>ChatGPT model (required)</label>
                  <select id={`${id}-model`} name="membershipModel" value={model ? choice.model : ""} required
                    onChange={(event) => membership.setChoice({ model: event.currentTarget.value, reasoningEffort: null })}>
                    <option value="">Choose a returned model</option>
                    {models.map((item) => <option key={item.slug} value={item.slug}>{item.displayName} ({item.slug})</option>)}
                  </select>
                  <label className="field" htmlFor={`${id}-effort`}>Reasoning effort</label>
                  <select id={`${id}-effort`} name="membershipEffort" value={choice.reasoningEffort ?? ""}
                    disabled={!model || (model.reasoningEfforts.length === 0 && !effortUnavailable)}
                    onChange={(event) => membership.setChoice({ ...choice, reasoningEffort: event.currentTarget.value || null })}>
                    <option value="">Provider default (no override)</option>
                    {effortUnavailable && <option value={choice.reasoningEffort!} disabled>Previous effort unavailable — choose again</option>}
                    {model?.reasoningEfforts.map((effort) => <option key={effort} value={effort}>{effort}</option>)}
                  </select>
                  {model?.reasoningEfforts.length === 0 && <p className="membership-help">This model returned no effort options. CrowClaw sends no reasoning effort override.</p>}
                  <button className="button button--primary" type="submit" disabled={busy}>{membership.pending === "use-model" ? "Applying model…" : "Use this model"}</button>
                </fieldset>
              </form>
            </>
          )}
        </div>
      )}
      {error && <div className="inline-error" role="alert" tabIndex={-1} ref={errorRef}>{error}</div>}
      {membership.notice && <p role="status">{membership.notice}</p>}
      {membership.pending === "sign-out" && <p role="status">Signing out and checking remote revocation…</p>}
      <MembershipUsage membership={membership} />
    </section>
  );
}

export function MembershipUsage({ membership }: { membership: MembershipController }) {
  return <div className="membership-usage">
    <button className="button button--secondary" type="button" disabled={membership.usagePending} onClick={() => void membership.manageUsage()}>Manage usage</button>
    {membership.usageError && <p role="alert">{membership.usageError}</p>}
  </div>;
}

export function MembershipWelcome({ membership }: { membership: MembershipController }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const id = useId();
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!membership.welcomeVisible || !dialog) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    return () => { dialog.close(); if (previous?.isConnected) previous.focus(); };
  }, [membership.welcomeVisible]);
  if (!membership.welcomeVisible) return null;
  return <dialog className="membership-welcome" ref={dialogRef} aria-labelledby={`${id}-title`} aria-describedby={`${id}-description`}
    onCancel={(event) => { event.preventDefault(); void membership.acknowledgeWelcome(); }}>
    <h2 id={`${id}-title`}>You're using your ChatGPT plan</h2>
    <p id={`${id}-description`}>Eligible AI requests in CrowClaw use your ChatGPT plan. You can manage usage in ChatGPT settings.</p>
    {membership.welcomeError && <p role="alert">{membership.welcomeError}</p>}
    <button className="button button--primary" type="button" autoFocus disabled={membership.acknowledging} onClick={() => void membership.acknowledgeWelcome()}>Got it</button>
  </dialog>;
}
