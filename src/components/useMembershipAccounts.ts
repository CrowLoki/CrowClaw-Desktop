import { useCallback, useEffect, useRef, useState } from "react";
import type { CrowClawGateway, MembershipAccount, MembershipModelRequest, ModelConnection } from "../gateway/contracts";

export type MembershipChoice = { model: string; reasoningEffort: string | null };
type SignInOperation = {
  requestId: string;
  saved: boolean;
  cancellation?: Promise<boolean>;
  completion?: Promise<void>;
};

function errorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : "The ChatGPT account operation failed. Please try again.";
}

export function membershipModels(account: MembershipAccount | undefined) {
  return account?.hasCredentials && account.catalog?.accountId === account.id ? account.catalog.models : [];
}

export function validMembershipChoice(account: MembershipAccount | undefined, choice: MembershipChoice) {
  const model = membershipModels(account).find(({ slug }) => slug === choice.model);
  return !!model && (choice.reasoningEffort === null || model.reasoningEfforts.includes(choice.reasoningEffort));
}

// Owned by App so navigation never abandons a pending native sign-in or its result.
export function useMembershipAccounts(
  gateway: CrowClawGateway,
  onConnection: (connection: ModelConnection) => void,
  onSignOut: (accountId: string) => void,
) {
  const [accounts, setAccounts] = useState<MembershipAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [welcomeAcknowledged, setWelcomeAcknowledged] = useState(true);
  const [acknowledging, setAcknowledging] = useState(false);
  const [welcomeError, setWelcomeError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const [signInBrowser,setSignInBrowser] = useState<'system'|'edge'>('system');
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [choices, setChoices] = useState<Record<string, MembershipChoice>>({});
  const [pending, setPending] = useState<"sign-in" | "sign-out" | "use-model" | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [refreshing, setRefreshing] = useState<Record<string, boolean>>({});
  const [catalogErrors, setCatalogErrors] = useState<Record<string, string | null>>({});
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [usagePending, setUsagePending] = useState(false);
  const [usageError, setUsageError] = useState<string | null>(null);
  const selectedRef = useRef("");
  const busyRef = useRef(false);
  const signInRef = useRef<SignInOperation | null>(null);
  const refreshVersions = useRef(new Map<string, number>());

  const readSnapshot = useCallback(async () => {
    const snapshot = await gateway.membershipSnapshot();
    setAccounts(snapshot.accounts);
    setWelcomeAcknowledged(snapshot.welcomeAcknowledged);
    setLoaded(true);
  }, [gateway]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    // Snapshot is local only. Mounting never signs in, refreshes models or runs inference.
    void gateway.membershipSnapshot().then((snapshot) => {
      if (!active) return;
      setAccounts(snapshot.accounts);
      setWelcomeAcknowledged(snapshot.welcomeAcknowledged);
      setLoaded(true);
    }).catch((cause) => { if (active) setError(errorMessage(cause)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [gateway]);

  function upsert(account: MembershipAccount) {
    setAccounts((current) => current.some(({ id }) => id === account.id)
      ? current.map((item) => item.id === account.id ? account : item)
      : [...current, account]);
  }

  async function refresh(accountId: string) {
    const version = (refreshVersions.current.get(accountId) ?? 0) + 1;
    refreshVersions.current.set(accountId, version);
    setRefreshing((current) => ({ ...current, [accountId]: true }));
    setCatalogErrors((current) => ({ ...current, [accountId]: null }));
    try {
      const account = await gateway.refreshMembershipModels(accountId);
      if (refreshVersions.current.get(accountId) !== version) return;
      if (account.id !== accountId || (account.catalog && account.catalog.accountId !== accountId)) {
        throw new Error("The returned model catalog does not belong to this saved account.");
      }
      upsert(account);
    } catch (cause) {
      if (refreshVersions.current.get(accountId) === version) {
        setCatalogErrors((current) => ({ ...current, [accountId]: errorMessage(cause) }));
      }
    } finally {
      if (refreshVersions.current.get(accountId) === version) {
        setRefreshing((current) => ({ ...current, [accountId]: false }));
      }
    }
  }

  function select(accountId: string) {
    if (busyRef.current || cancelling) return;
    selectedRef.current = accountId;
    setSelectedId(accountId);
    setError(null);
    setNotice(null);
    const account = accounts.find(({ id }) => id === accountId);
    if (account?.hasCredentials) void refresh(account.id);
  }

  const account = accounts.find(({ id }) => id === selectedId);
  const label = labels[selectedId] ?? account?.label ?? "";
  const savedChoice = account && account.selection?.accountId === account.id ? account.selection : null;
  const choice = choices[selectedId] ?? (savedChoice && validMembershipChoice(account, savedChoice)
    ? { model: savedChoice.model, reasoningEffort: savedChoice.reasoningEffort }
    : { model: "", reasoningEffort: null });

  async function signIn() {
    if (busyRef.current || cancelling || !loaded) return;
    if (!label.trim()) { setError("Enter a label for this saved registration."); return; }
    const accountId = selectedId || null;
    const operation: SignInOperation = { requestId: crypto.randomUUID(), saved: false };
    signInRef.current = operation;
    busyRef.current = true;
    setPending("sign-in");
    setError(null);
    setNotice("Complete sign-in in the browser opened by CrowClaw.");
    operation.completion = (async () => {
      try {
        const saved = await gateway.signInMembership({ requestId: operation.requestId, label: label.trim(), accountId, ...(signInBrowser==='edge'?{browser:'edge' as const}:{}) });
        if (accountId && saved.id !== accountId) throw new Error("Sign-in returned a different saved registration.");
        operation.saved = true;
        upsert(saved);
        setNotice(`Signed in as ${saved.label}. Choose a model to use this account.`);
        // Keep newer input edits and the new-registration form. Reconnect retains its selection.
        await refresh(saved.id);
      } catch (cause) {
        const cancelled = await operation.cancellation?.catch(() => false);
        if (!cancelled) { setNotice(null); setError(errorMessage(cause)); }
        else setNotice("Sign-in cancelled.");
      } finally {
        if (signInRef.current === operation) {
          signInRef.current = null;
          busyRef.current = false;
          setPending(null);
        }
      }
    })();
    await operation.completion;
  }

  async function cancelSignIn() {
    const operation = signInRef.current;
    if (!operation || operation.cancellation) return;
    setCancelling(true);
    setError(null);
    operation.cancellation = gateway.cancelMembershipSignIn(operation.requestId);
    try {
      const cancelled = await operation.cancellation;
      if (cancelled && !operation.saved) setNotice("Sign-in cancelled.");
      else {
        setNotice("Sign-in already finished. Checking the saved account.");
        await operation.completion;
        // Recover a saved registration even if the sign-in reply was lost.
        if (!operation.saved) await readSnapshot();
        setNotice("Sign-in already finished. Saved accounts are up to date.");
      }
    } catch (cause) {
      setError(errorMessage(cause));
      operation.cancellation = undefined;
    } finally { setCancelling(false); }
  }

  async function signOut() {
    if (!account || busyRef.current || cancelling || refreshing[account.id]) return;
    const accountId = account.id;
    busyRef.current = true;
    setPending("sign-out");
    setError(null);
    setNotice(null);
    refreshVersions.current.set(accountId, (refreshVersions.current.get(accountId) ?? 0) + 1);
    try {
      const result = await gateway.signOutMembership(accountId);
      if (result.account.id !== accountId) throw new Error("Sign-out returned a different saved registration.");
      upsert(result.account);
      onSignOut(accountId);
      setNotice(`${result.account.label}: signed out locally. ${result.remoteRevoked ? "Remote access revoked." : "Remote revocation was not confirmed."} ${result.detail}`);
    } catch (cause) { setError(errorMessage(cause)); }
    finally { busyRef.current = false; setPending(null); }
  }

  async function useModel() {
    if (busyRef.current || cancelling || !account || refreshing[account.id]) return;
    if (catalogErrors[account.id] || !validMembershipChoice(account, choice)) {
      setError("Choose a model and reasoning effort from this account's current catalog.");
      return;
    }
    const request: MembershipModelRequest = { accountId: account.id, ...choice };
    busyRef.current = true;
    setPending("use-model");
    setError(null);
    setNotice(null);
    try {
      const connection = await gateway.useMembershipModel(request);
      if (connection.provider !== "chatgpt" || connection.id !== `membership:${request.accountId}`) {
        throw new Error("The returned connection does not belong to this saved account.");
      }
      setAccounts((current) => current.map((item) => item.id === request.accountId ? { ...item, selection: request } : item));
      onConnection(connection);
      setNotice(`Using ${request.model} with ${account.label}.`);
    } catch (cause) { setError(errorMessage(cause)); }
    finally { busyRef.current = false; setPending(null); }
  }

  async function acknowledgeWelcome() {
    if (acknowledging) return;
    setAcknowledging(true);
    setWelcomeError(null);
    try {
      await gateway.acknowledgeMembershipWelcome();
      setWelcomeAcknowledged(true);
    } catch (cause) { setWelcomeError(errorMessage(cause)); }
    finally { setAcknowledging(false); }
  }

  async function manageUsage() {
    if (usagePending) return;
    setUsagePending(true);
    setUsageError(null);
    try { await gateway.manageMembershipUsage(); }
    catch (cause) { setUsageError(errorMessage(cause)); }
    finally { setUsagePending(false); }
  }

  async function retrySnapshot() {
    if (loading) return;
    setLoading(true);
    setError(null);
    try { await readSnapshot(); }
    catch (cause) { setError(errorMessage(cause)); }
    finally { setLoading(false); }
  }

  return {
    accounts, account, selectedId, select, label, choice, loading, loaded,
    pending, cancelling, refreshing: !!refreshing[selectedId], error, notice,
    catalogError: catalogErrors[selectedId], usagePending, usageError,
    welcomeVisible: !welcomeAcknowledged && accounts.some((item) => item.hasCredentials),
    acknowledging, welcomeError, acknowledgeWelcome, manageUsage, retrySnapshot,
    signIn, cancelSignIn, signOut, useModel,
    signInBrowser,setSignInBrowser,
    refresh: () => { if (account?.hasCredentials && !busyRef.current && !refreshing[selectedId]) void refresh(account.id); },
    setLabel: (value: string) => { setLabels((current) => ({ ...current, [selectedRef.current]: value })); setError(null); },
    setChoice: (value: MembershipChoice) => { setChoices((current) => ({ ...current, [selectedRef.current]: value })); setError(null); },
  };
}

export type MembershipController = ReturnType<typeof useMembershipAccounts>;
