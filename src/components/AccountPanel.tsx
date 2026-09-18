import { useCallback, useEffect, useState } from "react";
import { accountSignIn, accountSignOut, accountStatus, paymentsStartCheckout } from "../lib/commands";
import type { AccountStatus } from "../lib/types";
import { NOT_SIGNED_IN, SIGN_IN_CANCELLED, accountChanged, onAccountChanged, tierKey } from "../lib/account";
import { useUiLocale } from "../i18n";

type Busy = null | "signin" | "signout" | "checkout" | "refresh";

/**
 * Settings -> Account. The account is optional: signed out, the rest of JARVIS
 * works exactly as before. It is needed for Pro checkout.
 */
export default function AccountPanel() {
  const { t } = useUiLocale();
  const [status, setStatus] = useState<AccountStatus | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (refresh = false) => {
    try {
      setStatus(await accountStatus(refresh));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    return onAccountChanged(() => void load());
  }, [load]);

  async function run(kind: Exclude<Busy, null>, action: () => Promise<void>) {
    setBusy(kind);
    setError(null);
    setMessage(null);
    try {
      await action();
    } catch (e) {
      const msg = String(e);
      if (msg === NOT_SIGNED_IN) await load();
      // The user closed the browser or let it time out: not an error to report.
      if (msg !== SIGN_IN_CANCELLED) setError(msg);
    } finally {
      setBusy(null);
    }
  }

  const signIn = (provider?: "google") => run("signin", async () => {
    setStatus(await accountSignIn(provider));
    accountChanged();
  });
  const signOut = () => run("signout", async () => {
    await accountSignOut();
    await load();
    accountChanged();
  });
  const upgrade = () => run("checkout", async () => {
    await paymentsStartCheckout("pro");
    setMessage(t("upgradeOpened"));
  });
  const refresh = () => run("refresh", async () => {
    setStatus(await accountStatus(true));
    accountChanged();
  });

  if (!status) {
    return <div className="settings-panel"><p style={styles.hint}>{t("loading")}</p></div>;
  }
  if (!status.available) {
    return <div className="settings-panel"><p style={styles.hint}>{t("accountUnavailable")}</p></div>;
  }

  return (
    <div className="settings-panel">
      <h2 className="settings-heading">{t("accountSection")}</h2>
      {status.signed_in ? (
        <>
          <p style={styles.hint}>{t("signedInAs", { email: status.email ?? "-" })}</p>
          <p style={styles.hint}>{t("planLabel", { plan: t(tierKey(status.tier)) })}</p>
          <div style={styles.row}>
            {status.tier === "free" && (
              <button style={styles.button} disabled={busy !== null} onClick={() => void upgrade()}>{t("upgradePro")}</button>
            )}
            <button style={styles.button} disabled={busy !== null} onClick={() => void refresh()}>{busy === "refresh" ? t("refreshing") : t("refresh")}</button>
            <button style={styles.button} disabled={busy !== null} onClick={() => void signOut()}>{t("signOut")}</button>
          </div>
        </>
      ) : (
        <>
          <p style={styles.hint}>{t("accountSignedOut")}</p>
          <div style={styles.row}>
            <button style={styles.button} disabled={busy !== null} onClick={() => void signIn("google")}>{t("signInGoogle")}</button>
            <button style={styles.button} disabled={busy !== null} onClick={() => void signIn()}>{t("signInEmail")}</button>
          </div>
          {busy === "signin" && <p role="status" style={styles.hint}>{t("signingIn")}</p>}
        </>
      )}
      {message && <p role="status" style={styles.hint}>{message}</p>}
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
