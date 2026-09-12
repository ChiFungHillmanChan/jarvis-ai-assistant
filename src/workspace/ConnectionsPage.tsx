import { useUiLocale } from '../i18n';
import { useCallback, useEffect, useRef, useState } from "react";
import { googleStatus } from "../lib/commands";
import { mailWorkspace, type MailAccount } from "../lib/mail";
import { useWorkspace } from "./WorkspaceContext";
import "../pages/settings-workspace.css";

function syncLabel(value: string | null, locale: string): string {
  if (!value) return locale === "zh-HK" ? "尚未同步" : "Not synced yet";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return locale === "zh-HK" ? "未能讀取同步時間" : "Sync time unavailable";
  return locale === "zh-HK" ? `上次同步：${date.toLocaleString(locale, { dateStyle: "medium", timeStyle: "short" })}` : `Last synced ${date.toLocaleString(locale, { dateStyle: "medium", timeStyle: "short" })}`;
}

export default function ConnectionsPage() {
  const { locale, text } = useUiLocale();
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
          <h1>{text('Connections', '連接服務')}</h1>
          <p>{text('Manage the accounts and services available to JARVIS on this Mac.', '管理 JARVIS 喺呢部 Mac 可用嘅帳戶同服務。')}</p>
        </div>
        <button onClick={() => { void refresh(); }} disabled={refreshing}>
          {refreshing ? text("Refreshing…", "重新整理緊…") : text("Refresh status", "重新整理狀態")}
        </button>
      </header>

      <section className="connections-section" aria-labelledby="connections-mail-title">
        <div className="connections-section-head">
          <div>
            <h2 id="connections-mail-title">{text('Gmail workspaces', 'Gmail 工作區')}</h2>
            <p>{text('Bring several Gmail accounts into Mail for local copies, groups and source memories. Each account uses read-only Gmail access.', '將多個 Gmail 帳戶加入郵件工作區，建立本機副本、群組同來源記憶。每個帳戶只授權唯讀 Gmail 權限。')}</p>
          </div>
          <button className="connections-primary" onClick={() => open("email")}>{text('Manage in Mail', '喺郵件管理')}</button>
        </div>
        {mailError ? (
          <p role="alert" className="connections-message connections-error">{text('Could not read Mail account status. Refresh to try again, or open Mail in the desktop app.', '未能讀取郵件帳戶狀態。請重新整理，或者喺桌面 App 開啟郵件。')}</p>
        ) : accounts === null ? (
          <p role="status" className="connections-message">{text('Loading Mail accounts…', '載入郵件帳戶緊…')}</p>
        ) : accounts.length === 0 ? (
          <p className="connections-message">{text('No Gmail accounts added to Mail yet. Open Mail to connect your first account.', '尚未加入 Gmail 帳戶。請開啟郵件連接第一個帳戶。')}</p>
        ) : (
          <ul className="connections-account-list">
            {accounts.map((account) => (
              <li className="connections-account" key={account.id}>
                <div>
                  <strong>{account.email}</strong>
                  <p>{locale === "zh-HK" ? `本機有 ${account.message_count.toLocaleString(locale)} 封郵件。${syncLabel(account.last_synced, locale)}。` : `${account.message_count.toLocaleString(locale)} local ${account.message_count === 1 ? "message" : "messages"}. ${syncLabel(account.last_synced, locale)}.`}</p>
                </div>
                <span className={`connections-status${account.connected ? " is-connected" : ""}`}>
                  {account.connected ? text("Connected", "已連接") : text("Disconnected · Local copies kept", "已斷開 · 本機副本保留")}
                </span>
              </li>
            ))}
          </ul>
        )}
        <p className="connections-message">{text('Sync, add or disconnect accounts in Mail. Disconnecting keeps local copies; deleting an account is a separate action.', '可以喺郵件同步、加入或者斷開帳戶。斷開會保留本機副本；刪除帳戶係另一項操作。')}</p>
      </section>

      <section className="connections-section" aria-labelledby="connections-assistant-title">
        <div className="connections-section-head">
          <div>
            <h2 id="connections-assistant-title">{text('Google for Assistant and Calendar', '助手同日曆嘅 Google 連接')}</h2>
            <p>{text('This separate Google connection provides Assistant mail tools, Calendar and briefings. A Gmail account added to Mail does not connect these tools.', '呢個獨立 Google 連接供助手郵件工具、日曆同簡報使用。喺郵件加入 Gmail 帳戶唔會自動連接呢啲工具。')}</p>
          </div>
          <button onClick={() => open("settings")}>{text('Manage in Settings', '喺設定管理')}</button>
        </div>
        {googleError ? (
          <p role="alert" className="connections-message connections-error">{text('Could not read Assistant Google status. Refresh to try again.', '未能讀取助手 Google 狀態。請重新整理再試。')}</p>
        ) : (
          <p className="connections-message" role="status">
            {assistantGoogle === null ? text("Loading Google status…", "載入 Google 狀態緊…") : assistantGoogle ? text("Google is connected for Assistant and Calendar.", "Google 已連接到助手同日曆。") : text("Google is not connected for Assistant and Calendar.", "Google 尚未連接到助手同日曆。")}
          </p>
        )}
        <p className="connections-message">{text('Choose Assistant Google in Settings to connect, switch or disconnect this account.', '喺設定嘅「助手 Google」連接、切換或者斷開帳戶。')}</p>
      </section>

      <section className="connections-section" aria-labelledby="connections-tools-title">
        <div className="connections-section-head">
          <div>
            <h2 id="connections-tools-title">{text('Other services', '其他服務')}</h2>
            <p>{text('Manage access tokens and API keys in Settings. Saving a token does not verify access to a service.', '喺設定管理 token 同 API 密鑰。儲存 token 唔代表已驗證服務存取。')}</p>
          </div>
        </div>
        <div className="connections-services">
          {[
            { name: "GitHub", detail: text("Personal access token for GitHub tools.", "GitHub 工具嘅個人存取 token。") },
            { name: "Notion", detail: text("Integration token for pages shared with your integration.", "供應用程式存取已分享頁面嘅 integration token。") },
            { name: "Obsidian", detail: text("Local REST API key and endpoint for your vault.", "資料庫嘅 Local REST API 密鑰同端點。") },
            { name: text("AI providers", "AI 供應商"), detail: text("OpenAI, Gemini, Claude and AWS Bedrock credentials.", "OpenAI、Gemini、Claude 同 AWS Bedrock 憑證。") },
          ].map((service) => (
            <div key={service.name} className="connections-service">
              <div><h3>{service.name}</h3><p>{service.detail}</p></div>
              <button aria-label={locale === "zh-HK" ? `喺設定管理 ${service.name}` : `Manage ${service.name} in Settings`} onClick={() => open("settings")}>{text('Manage in Settings', '喺設定管理')}</button>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
