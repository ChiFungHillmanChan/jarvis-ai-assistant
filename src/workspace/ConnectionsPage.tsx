import { useCallback, useEffect, useRef, useState } from "react";
import { googleStatus } from "../lib/commands";
import { mailWorkspace, type MailAccount } from "../lib/mail";
import { useWorkspace } from "./WorkspaceContext";
import "../pages/settings-workspace.css";

function syncLabel(value: string | null): string {
  if (!value) return "Not synced yet";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Sync time unavailable";
  return `Last synced ${date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}`;
}

export default function ConnectionsPage() {
  const { open, state } = useWorkspace();
  const [accounts, setAccounts] = useState<MailAccount[] | null>(null);
  const [assistantGoogle, setAssistantGoogle] = useState<boolean | null>(null);
  const [mailError, setMailError] = useState(false);
  const [googleError, setGoogleError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const pending = useRef(false);
  const mounted = useRef(false);

  const refresh = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    setRefreshing(true);
    const [mail, google] = await Promise.allSettled([mailWorkspace(), googleStatus()]);
    if (mounted.current) {
      // A failed status check is unknown, never an assumed disconnected account.
      setAccounts(mail.status === "fulfilled" ? mail.value.accounts : null);
      setMailError(mail.status === "rejected");
      setAssistantGoogle(google.status === "fulfilled" ? google.value : null);
      setGoogleError(google.status === "rejected");
      setRefreshing(false);
    }
    pending.current = false;
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  // Tabs stay mounted. Re-read native status when this tab is visited again.
  const active = state.tabs.find((tab) => tab.id === state.activeId)?.view === "connections";
  useEffect(() => {
    if (!active) return;
    void refresh();
    const onFocus = () => { void refresh(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [active, refresh]);

  return (
    <div className="connections-page">
      <header className="connections-header">
        <div>
          <h1>Connections</h1>
          <p>Manage the accounts and services available to JARVIS on this Mac.</p>
        </div>
        <button onClick={() => { void refresh(); }} disabled={refreshing}>
          {refreshing ? "Refreshing…" : "Refresh status"}
        </button>
      </header>

      <section className="connections-section" aria-labelledby="connections-mail-title">
        <div className="connections-section-head">
          <div>
            <h2 id="connections-mail-title">Gmail workspaces</h2>
            <p>Bring several Gmail accounts into Mail for local copies, groups and source memories. Each account uses read-only Gmail access.</p>
          </div>
          <button className="connections-primary" onClick={() => open("email")}>Manage in Mail</button>
        </div>
        {mailError ? (
          <p role="alert" className="connections-message connections-error">Could not read Mail account status. Refresh to try again, or open Mail in the desktop app.</p>
        ) : accounts === null ? (
          <p role="status" className="connections-message">Loading Mail accounts…</p>
        ) : accounts.length === 0 ? (
          <p className="connections-message">No Gmail accounts added to Mail yet. Open Mail to connect your first account.</p>
        ) : (
          <ul className="connections-account-list">
            {accounts.map((account) => (
              <li className="connections-account" key={account.id}>
                <div>
                  <strong>{account.email}</strong>
                  <p>{account.message_count.toLocaleString()} local {account.message_count === 1 ? "message" : "messages"}. {syncLabel(account.last_synced)}.</p>
                </div>
                <span className={`connections-status${account.connected ? " is-connected" : ""}`}>
                  {account.connected ? "Connected" : "Disconnected · Local copies kept"}
                </span>
              </li>
            ))}
          </ul>
        )}
        <p className="connections-message">Sync, add or disconnect accounts in Mail. Disconnecting keeps local copies; deleting an account is a separate action.</p>
      </section>

      <section className="connections-section" aria-labelledby="connections-assistant-title">
        <div className="connections-section-head">
          <div>
            <h2 id="connections-assistant-title">Google for Assistant and Calendar</h2>
            <p>This separate Google connection provides Assistant mail tools, Calendar and briefings. A Gmail account added to Mail does not connect these tools.</p>
          </div>
          <button onClick={() => open("settings")}>Manage in Settings</button>
        </div>
        {googleError ? (
          <p role="alert" className="connections-message connections-error">Could not read Assistant Google status. Refresh to try again.</p>
        ) : (
          <p className="connections-message" role="status">
            {assistantGoogle === null ? "Loading Google status…" : assistantGoogle ? "Google is connected for Assistant and Calendar." : "Google is not connected for Assistant and Calendar."}
          </p>
        )}
        <p className="connections-message">Choose Assistant Google in Settings to connect, switch or disconnect this account.</p>
      </section>

      <section className="connections-section" aria-labelledby="connections-tools-title">
        <div className="connections-section-head">
          <div>
            <h2 id="connections-tools-title">Other services</h2>
            <p>Manage access tokens and API keys in Settings. Saving a token does not verify access to a service.</p>
          </div>
        </div>
        <div className="connections-services">
          {[
            { name: "GitHub", detail: "Personal access token for GitHub tools." },
            { name: "Notion", detail: "Integration token for pages shared with your integration." },
            { name: "Obsidian", detail: "Local REST API key and endpoint for your vault." },
            { name: "AI providers", detail: "OpenAI, Gemini, Claude and AWS Bedrock credentials." },
          ].map((service) => (
            <div key={service.name} className="connections-service">
              <div><h3>{service.name}</h3><p>{service.detail}</p></div>
              <button aria-label={`Manage ${service.name} in Settings`} onClick={() => open("settings")}>Manage in Settings</button>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
