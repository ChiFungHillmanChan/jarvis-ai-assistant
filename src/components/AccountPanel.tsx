import { useCallback, useEffect, useRef, useState } from "react";
import { accountSignIn, accountSignOut, accountStatus, paymentsOpenPortal, paymentsReopenUrl } from "../lib/commands";
import type { AccountStatus, PaymentPage } from "../lib/types";
import { NOT_SIGNED_IN, SIGN_IN_CANCELLED, accountChanged, onAccountChanged, createAccountOperationGuard, isAccountSessionChanging, onAccountSessionChange, tierKey } from "../lib/account";
import { createAccountMonitor, type PaymentState } from "../lib/account-monitor";
import { formatPlanDate, planLine } from "../lib/paywall";
import { useUiLocale } from "../i18n";
import { confirmAction } from "./ConfirmDialog";
import type { MessageKey } from "../i18n/catalog";

type Busy = null | "signin" | "signout" | "checkout" | "portal" | "reopen" | "refresh";

/** Plan details, billing management and sign-out. Subscribing happens in the paywall gate. */
export default function AccountPanel() {
  const { t, locale } = useUiLocale();
  const [status, setStatus] = useState<AccountStatus | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [sessionChanging, setSessionChanging] = useState(isAccountSessionChanging);
  const operations = useRef(createAccountOperationGuard());
  const [message, setMessage] = useState<MessageKey | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [payment, setPayment] = useState<PaymentState>(null);
  const [page, setPage] = useState<(PaymentPage & { kind: "checkout" | "portal" }) | null>(null);
  const monitor = useRef<ReturnType<typeof createAccountMonitor> | null>(null);
  const load = useCallback((refresh = false) => monitor.current?.refresh(refresh) ?? Promise.resolve(), []);

  useEffect(() => {
    const watcher = createAccountMonitor({
      fetchStatus: accountStatus,
      onStatus: next => {
        setStatus(next);
        setError(null);
        const paid = next.tier === "pro" || next.tier === "team";
        setPayment(previous => (previous === "confirmed" && !paid) || (previous === "unconfirmed" && paid) ? null : previous);
        if (paid) {
          setPage(previous => previous?.kind === "checkout" ? null : previous);
          setMessage(previous => previous === "upgradeOpened" ? null : previous);
        }
        accountChanged(next);
      },
      onPayment: setPayment,
      onError: e => setError(String(e)),
    });
    monitor.current = watcher;
    void watcher.refresh();
    const unsubscribe = onAccountChanged(next => {
      if (!next) { void watcher.refresh(); return; }
      setStatus(next);
      if (!next.signed_in) { watcher.cancelPayment(); setPage(null); setMessage(null); }
    });
    const stopSession = onAccountSessionChange(changing => {
      setSessionChanging(changing);
      if (changing) {
        operations.current.invalidate();
        watcher.cancelPayment();
        setPage(null); setMessage(null); setError(null); setBusy(null);
      } else void watcher.refresh(true);
    });
    // Returning from Stripe rechecks immediately, including changes in the portal.
    const resume = () => { if (document.visibilityState !== "hidden") void watcher.refresh(true); };
    window.addEventListener("focus", resume);
    document.addEventListener("visibilitychange", resume);
    return () => {
      unsubscribe();
      stopSession();
      operations.current.invalidate();
      window.removeEventListener("focus", resume);
      document.removeEventListener("visibilitychange", resume);
      watcher.dispose();
      monitor.current = null;
    };
  }, []);

  async function run(kind: Exclude<Busy, null>, action: (current: () => boolean) => Promise<void>) {
    operations.current.invalidate();
    setBusy(kind);
    setError(null);
    setMessage(null);
    let current = () => false;
    try {
      // Sign-in/out emits its boundary synchronously. Capture afterwards so
      // this operation survives its own boundary but never a later one.
      const pending = action(() => current());
      current = operations.current.capture();
      setBusy(kind);
      await pending;
    } catch (e) {
      if (!current()) return;
      const msg = String(e);
      if (msg === NOT_SIGNED_IN) {
        monitor.current?.cancelPayment();
        setPage(null);
        await load(true);
        if (current()) setError(t("sessionEnded"));
      } else if (msg === "billing_busy") {
        setError(t("billingBusy"));
      } else if (msg !== SIGN_IN_CANCELLED) {
        setError(msg);
      }
    } finally {
      if (current()) setBusy(null);
    }
  }

  const signIn = (provider?: "google") => run("signin", async current => {
    monitor.current?.cancelPayment();
    const next = await accountSignIn(provider);
    if (!current()) return;
    setStatus(next);
    accountChanged(next);
  });
  // Spec 6.3: sign out asks first. It ends the account session (the refresh
  // token and cached plan in the Keychain) and opens the browser logout page;
  // nothing else on this Mac is removed.
  const signOut = async () => {
    if (!(await confirmAction(t("signOutConfirm"), t("signOut")))) return;
    await endSession();
  };
  const endSession = () => run("signout", async current => {
    monitor.current?.cancelPayment();
    setPage(null);
    await accountSignOut();
    if (current()) await load(true);
  });
  const showPage = (next: PaymentPage, kind: "checkout" | "portal") => {
    setPage({ ...next, kind });
    setMessage(next.browser_opened ? (kind === "checkout" ? "upgradeOpened" : "billingOpened") : "billingBrowserFailed");
  };
  const manageBilling = () => run("portal", async current => {
    monitor.current?.cancelPayment();
    const next = await paymentsOpenPortal(locale);
    if (current()) showPage(next, "portal");
  });
  const reopen = () => run("reopen", async current => {
    if (!page) return;
    const next = await paymentsReopenUrl(page.url, page.session_generation);
    if (!current()) return;
    showPage(next, page.kind);
    if (page.kind === "checkout" && next.browser_opened) monitor.current?.waitForPayment();
  });
  const refresh = () => run("refresh", async () => { await load(true); });

  if (!status) {
    if (error) {
      return (
        <div className="settings-panel">
          <p role="alert" style={styles.error}>{error}</p>
          <button style={styles.button} onClick={() => { setError(null); void load(); }}>{t("retry")}</button>
        </div>
      );
    }
    return <div className="settings-panel"><p style={styles.hint}>{t("loading")}</p></div>;
  }
  if (!status.available) {
    return <div className="settings-panel"><p style={styles.hint}>{t("accountUnavailable")}</p></div>;
  }
  const line = planLine(status);

  return (
    <div className="settings-panel">
      <h2 className="settings-heading">{t("accountSection")}</h2>
      <p style={styles.hint}>{t("billingByok")}</p>
      {status.signed_in ? (
        <>
          <p style={styles.hint}>{t("signedInAs", { email: status.email ?? "-" })}</p>
          <p style={styles.hint}>{t("planLabel", { plan: t(tierKey(status.tier)) })}</p>
          {line && <p style={styles.hint}>{t(line.key, { date: formatPlanDate(line.date, locale) })}</p>}
          <div style={styles.row}>
            {(status.tier === "pro" || status.tier === "team") && (
              <button style={styles.button} disabled={sessionChanging || busy !== null} onClick={() => void manageBilling()}>{t("manageBilling")}</button>
            )}
            <button style={styles.button} disabled={sessionChanging || busy !== null} onClick={() => void refresh()}>{busy === "refresh" ? t("refreshing") : t("refresh")}</button>
            <button style={styles.button} disabled={sessionChanging || busy !== null} onClick={() => void signOut()}>{t("signOut")}</button>
          </div>
        </>
      ) : (
        <>
          <p style={styles.hint}>{t("accountSignedOut")}</p>
          <div style={styles.row}>
            <button style={styles.button} disabled={sessionChanging || busy !== null} onClick={() => void signIn("google")}>{t("signInGoogle")}</button>
            <button style={styles.button} disabled={sessionChanging || busy !== null} onClick={() => void signIn()}>{t("signInEmail")}</button>
          </div>
          {busy === "signin" && <p role="status" style={styles.hint}>{t("signingIn")}</p>}
        </>
      )}
      {message && payment !== "confirmed" && <p role="status" style={styles.hint}>{t(message)}</p>}
      {payment && <p role="status" style={styles.hint}>{t(payment === "waiting" ? "paymentChecking" : payment === "confirmed" ? "paymentConfirmed" : "paymentUnconfirmed")}</p>}
      {page && payment !== "confirmed" && (
        <button style={styles.button} disabled={sessionChanging || busy !== null} onClick={() => void reopen()}>{t(page.kind === "checkout" ? "reopenCheckout" : "reopenBilling")}</button>
      )}
      {error && <p role="alert" style={styles.error}>{error}</p>}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  hint: { color: "var(--text-secondary)", fontSize: 13, marginBottom: 12, lineHeight: 1.6, maxWidth: "72ch" },
  row: { display: "flex", gap: 8, flexWrap: "wrap", marginTop: 4 },
  button: { background: "var(--surface-solid)", border: "1px solid var(--border-primary)", borderRadius: 6, padding: "8px 12px", color: "var(--text-primary)", cursor: "pointer", fontFamily: "inherit", fontSize: 13, whiteSpace: "nowrap" },
  error: { color: "var(--danger, #ff6b6b)", fontSize: 13, marginTop: 12, lineHeight: 1.6 },
};
