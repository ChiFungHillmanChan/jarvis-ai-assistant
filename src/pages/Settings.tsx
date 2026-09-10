import { useEffect, useState } from "react";
import {
  getSettings,
  updateSetting,
  googleConnect,
  googleStatus,
  googleDisconnect,
  saveNotionToken,
  saveGitHubToken,
  saveObsidianKey,
  getVoiceSettings,
  setVoiceSetting,
  listTtsVoices,
  getWakeWordStatus,
  enableWakeWord,
  disableWakeWord,
  isModelDownloaded,
  downloadModel,
  getWallpaperStatus,
  enableWallpaper,
  disableWallpaper,
  listLocalEndpoints,
  addLocalEndpoint,
  removeLocalEndpoint,
  testEndpointConnection,
  listEndpointModels,
  getProviderChain,
  updateProviderChain,
  setAwsCredentials,
  getAwsCredentialsStatus,
  clearAwsCredentials,
  getPreference,
  setPreference,
  setApiKey,
  getApiKeyStatus,
} from "../lib/commands";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { VoiceSettings, WakeWordStatus, VoiceState, LocalEndpoint, LocalModel, ProviderChainEntry, EndpointHealth } from "../lib/types";
import { GOOGLE_ALREADY_CONNECTED } from "../lib/types";
import { useVoiceState } from "../hooks/useVoiceState";
import LocalAiSetup from "../components/LocalAiSetup";
import { useWorkspace, useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./settings-workspace.css";
import { Cpu, Key, Globe, Gem, Mic, Bot, Palette, GithubMark, NotionMark } from "../components/icons";

const API_KEY_PROVIDERS = [
  { id: "gemini", label: "Google Gemini", help: "Gemini chat and speech-to-text" },
  { id: "claude", label: "Anthropic Claude", help: "Claude chat and fallback" },
  { id: "openai", label: "OpenAI", help: "OpenAI chat and Mail analysis" },
] as const;

// Must match gemini::MODEL_PRO / MODEL_FLASH in src-tauri/src/ai/gemini.rs. A
// chain entry's model_id is sent to the API verbatim, so a stale id here is a
// 404 on every request that reaches the entry.
const GEMINI_MODEL_PRO = "gemini-3.1-pro-preview";
const GEMINI_MODEL_FLASH = "gemini-3.7-flash";

// Deleting a saved key is its own IPC call: set_api_key refuses a blank value
// unless `remove` is set, so an accidental SAVE on the (permanently blank) key
// field can no longer destroy a working credential. src/lib/commands.ts has no
// wrapper for the removal form yet, hence the direct invoke -- the arg names
// are single lowercase words, so its camelCase normalisation is a no-op here.
function clearApiKey(provider: string): Promise<void> {
  return invoke("set_api_key", { provider, value: "", remove: true });
}

export default function Settings() {
  const { appearance, setAppearance, open } = useWorkspace();
  const [aiProvider, setAiProvider] = useState("gemini_primary");
  const [autoBriefing, setAutoBriefing] = useState(false);
  const [apiKeyDrafts, setApiKeyDrafts] = useState<Record<string, string>>({});
  const [apiKeyStatus, setApiKeyStatus] = useState<Record<string, boolean>>({});
  const [apiKeyError, setApiKeyError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [googleConnected, setGoogleConnected] = useState<boolean | null>(null);
  const [googleAccount, setGoogleAccount] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  const [googleError, setGoogleError] = useState<string | null>(null);
  const [googleNotice, setGoogleNotice] = useState<string | null>(null);
  const [notionToken, setNotionToken] = useState("");
  const [githubToken, setGithubToken] = useState("");
  const [notionSaved, setNotionSaved] = useState(false);
  const [githubSaved, setGithubSaved] = useState(false);
  const [obsidianKey, setObsidianKey] = useState("");
  const [obsidianSaved, setObsidianSaved] = useState(false);
  const [integrationError, setIntegrationError] = useState<string | null>(null);
  const [obsidianBaseUrl, setObsidianBaseUrl] = useState("");
  const [voiceSettings, setVoiceSettingsState] = useState<VoiceSettings | null>(null);
  const [ttsVoices, setTtsVoices] = useState<string[]>([]);
  const [wakeStatus, setWakeStatus] = useState<WakeWordStatus | null>(null);
  const [wakeBusy, setWakeBusy] = useState(false);
  const [wakeError, setWakeError] = useState<string | null>(null);
  const { state: voiceState } = useVoiceState();
  const [wallpaperActive, setWallpaperActive] = useState(false);
  const [wallpaperBusy, setWallpaperBusy] = useState(false);
  const [wallpaperError, setWallpaperError] = useState<string | null>(null);

  useEffect(() => {
    getApiKeyStatus().then(setApiKeyStatus).catch(() => {});
  }, []);

  // Absent row throws, which just means "still on the plugin default".
  useEffect(() => {
    getPreference("obsidian_base_url")
      .then((v) => { if (v) setObsidianBaseUrl(v); })
      .catch(() => {});
  }, []);

  async function handleSaveApiKey(provider: string) {
    setApiKeyError(null);
    // The field is blank by design for a configured provider -- a saved key is
    // never read back. Saving that blank must do nothing at all; removal is the
    // separate CLEAR button below.
    const draft = (apiKeyDrafts[provider] ?? "").trim();
    if (!draft) return;
    try {
      await setApiKey(provider, draft);
      setApiKeyDrafts((d) => ({ ...d, [provider]: "" }));
      setApiKeyStatus(await getApiKeyStatus());
    } catch (e) {
      setApiKeyError(String(e));
    }
  }

  async function handleClearApiKey(provider: string, label: string) {
    setApiKeyError(null);
    if (!window.confirm(`Remove the saved ${label} key from your Keychain? This cannot be undone.`)) {
      return;
    }
    try {
      await clearApiKey(provider);
      setApiKeyDrafts((d) => ({ ...d, [provider]: "" }));
      setApiKeyStatus(await getApiKeyStatus());
    } catch (e) {
      setApiKeyError(String(e));
    }
  }

  // AWS Bedrock credentials state
  const [awsAccessKey, setAwsAccessKey] = useState("");
  const [awsSecret, setAwsSecret] = useState("");
  const [awsRegion, setAwsRegion] = useState("ap-southeast-1");
  const [awsError, setAwsError] = useState<string | null>(null);
  const [awsStatus, setAwsStatus] = useState<{ configured: boolean; region: string | null }>(
    { configured: false, region: null }
  );

  // Language & Voice prefs
  const [language, setLanguage] = useState("en");
  const [sttLanguage, setSttLanguage] = useState("auto");
  const [ttsVoiceYue, setTtsVoiceYue] = useState("Sinji");

  // Local LLM state
  const [endpoints, setEndpoints] = useState<LocalEndpoint[]>([]);
  const [chain, setChain] = useState<ProviderChainEntry[]>([]);
  const [newEndpointUrl, setNewEndpointUrl] = useState("");
  const [newEndpointName, setNewEndpointName] = useState("");
  const [endpointBusy, setEndpointBusy] = useState(false);
  const [endpointModels, setEndpointModels] = useState<Record<string, LocalModel[]>>({});
  const [endpointHealth, setEndpointHealth] = useState<Record<string, EndpointHealth>>({});
  const [llmError, setLlmError] = useState<string | null>(null);
  const [section, setSection] = useWorkspaceValue<string>("section", "ai");

  async function refreshSettings() {
    setLoadError(false);
    try {
      const s = await getSettings();
      setAiProvider(s.values["ai_provider"] || "gemini_primary");
      // Same rule as the startup reader in lib.rs: an absent row means off.
      setAutoBriefing(s.values["auto_briefing"] === "true");
      setLoaded(true);
    } catch {
      setLoadError(true);
    }
  }

  useEffect(() => { void refreshSettings(); }, []);

  useEffect(() => {
    void refreshGoogle();
  }, []);

  useEffect(() => {
    getVoiceSettings().then(setVoiceSettingsState).catch(() => setWakeError("Could not load voice settings."));
    listTtsVoices().then(setTtsVoices).catch(() => setWakeError("Could not load the available voices."));
    void refreshWakeStatus();
  }, []);

  useEffect(() => {
    listLocalEndpoints().then(setEndpoints).catch(() => {});
    getProviderChain().then(setChain).catch(() => {});
  }, []);

  useEffect(() => {
    getWallpaperStatus().then(setWallpaperActive).catch(() => {});
    const unlisten = listen<boolean>("wallpaper-status", (event) => {
      setWallpaperActive(event.payload);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    getAwsCredentialsStatus().then((status) => {
      setAwsStatus(status);
      // Without this the select keeps its hardcoded default and disagrees with
      // the region the saved credentials actually use.
      if (status.region) setAwsRegion(status.region);
    }).catch(console.error);
  }, []);

  useEffect(() => {
    (async () => {
      try {
        setLanguage(await getPreference("language"));
        setSttLanguage(await getPreference("stt_language"));
        setTtsVoiceYue(await getPreference("tts_voice_yue"));
      } catch (e) {
        console.error("load language prefs:", e);
      }
    })();
  }, []);

  async function refreshWakeStatus() {
    try {
      const [status, downloaded] = await Promise.all([
        getWakeWordStatus(),
        isModelDownloaded(),
      ]);
      setWakeStatus({ ...status, model_downloaded: downloaded });
    } catch (e) {
      setWakeError(String(e));
    }
  }

  async function handleAddEndpoint() {
    if (!newEndpointUrl.trim()) return;
    setEndpointBusy(true);
    setLlmError(null);
    try {
      const ep = await addLocalEndpoint(
        newEndpointName.trim() || newEndpointUrl.trim(),
        newEndpointUrl.trim()
      );
      setEndpoints((prev) => [...prev, ep]);
      setNewEndpointUrl("");
      setNewEndpointName("");
      // Fetch models for new endpoint
      const models = await listEndpointModels(ep.id);
      setEndpointModels((prev) => ({ ...prev, [ep.id]: models }));
    } catch (e) {
      setLlmError(String(e));
    } finally {
      setEndpointBusy(false);
    }
  }

  async function handleRemoveEndpoint(id: string) {
    try {
      await removeLocalEndpoint(id);
      setEndpoints((prev) => prev.filter((e) => e.id !== id));
      // Remove from chain too
      const newChain = chain.filter((c) => c.endpoint_id !== id);
      setChain(newChain);
      await updateProviderChain(newChain);
    } catch (e) {
      setLlmError(String(e));
    }
  }

  async function handleTestEndpoint(id: string) {
    setLlmError(null);
    try {
      const health = await testEndpointConnection(id);
      setEndpointHealth((prev) => ({ ...prev, [id]: health }));
      // Refresh models
      const models = await listEndpointModels(id);
      setEndpointModels((prev) => ({ ...prev, [id]: models }));
    } catch (e) {
      setLlmError(String(e));
    }
  }

  async function handleAddToChain(providerType: string, endpointId?: string, modelId?: string) {
    const newEntry: ProviderChainEntry = {
      position: chain.length,
      provider_type: providerType as ProviderChainEntry["provider_type"],
      endpoint_id: endpointId,
      model_id: modelId,
      enabled: true,
    };
    const updated = [...chain, newEntry];
    setChain(updated);
    try {
      await updateProviderChain(updated);
    } catch (e) {
      setLlmError(String(e));
    }
  }

  async function handleRemoveFromChain(index: number) {
    const updated = chain.filter((_, i) => i !== index).map((c, i) => ({ ...c, position: i }));
    setChain(updated);
    try {
      await updateProviderChain(updated);
    } catch (e) {
      setLlmError(String(e));
    }
  }

  async function handleMoveChainEntry(index: number, direction: "up" | "down") {
    const swapIdx = direction === "up" ? index - 1 : index + 1;
    if (swapIdx < 0 || swapIdx >= chain.length) return;
    const updated = [...chain];
    [updated[index], updated[swapIdx]] = [updated[swapIdx], updated[index]];
    const reindexed = updated.map((c, i) => ({ ...c, position: i }));
    setChain(reindexed);
    try {
      await updateProviderChain(reindexed);
    } catch (e) {
      setLlmError(String(e));
    }
  }

  async function handleToggleChainEntry(index: number) {
    const updated = chain.map((c, i) => i === index ? { ...c, enabled: !c.enabled } : c);
    setChain(updated);
    try {
      await updateProviderChain(updated);
    } catch (e) {
      setLlmError(String(e));
    }
  }

  function handleProviderChange(value: string) {
    setAiProvider(value);
    void updateSetting("ai_provider", value);
  }

  // Connection state is always read back from the backend rather than assumed,
  // so DISCONNECT and SWITCH ACCOUNT settle on the truth without a relaunch.
  // The account label is the Google profile name persisted at OAuth
  // (user_preferences.user_name); disconnect deletes that row, so an absent row
  // correctly reads as "no account".
  async function refreshGoogle() {
    let connected = false;
    try {
      connected = await googleStatus();
    } catch (e) {
      setGoogleError(String(e));
      setGoogleConnected(null);
      setGoogleAccount(null);
      return;
    }
    setGoogleConnected(connected);
    if (!connected) {
      setGoogleAccount(null);
      return;
    }
    try {
      const name = (await getPreference("user_name")).trim();
      setGoogleAccount(name || null);
    } catch {
      setGoogleAccount(null);
    }
  }

  async function handleGoogleConnect(force: boolean) {
    // Switching revokes the current grant and wipes the cached mail before the
    // new flow starts, so it gets the same confirm-then-act treatment as CLEAR.
    if (force && !window.confirm(
      "Switch Assistant Google account? This revokes this Google access and deletes the Assistant mail cache, Calendar events and briefings on this Mac. Mail workspace copies are kept."
    )) {
      return;
    }
    setConnecting(true);
    setGoogleError(null);
    setGoogleNotice(null);
    try {
      const result = await googleConnect(force);
      setGoogleNotice(
        result === GOOGLE_ALREADY_CONNECTED
          ? "That account is already linked. Use Switch account to change it."
          : result
      );
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      await refreshGoogle();
      setConnecting(false);
    }
  }

  async function handleGoogleDisconnect() {
    if (!window.confirm(
      "Disconnect Assistant Google? This revokes this Google access and deletes the Assistant mail cache, Calendar events and briefings on this Mac. Mail workspace copies are kept. You can reconnect later."
    )) {
      return;
    }
    setDisconnecting(true);
    setGoogleError(null);
    setGoogleNotice(null);
    try {
      setGoogleNotice(await googleDisconnect());
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      // A failed disconnect can still have cleared part of the session, so the
      // displayed state comes from the backend either way.
      await refreshGoogle();
      setDisconnecting(false);
    }
  }

  async function handleSaveNotion() {
    if (!notionToken.trim()) return;
    setIntegrationError(null);
    try {
      await saveNotionToken(notionToken.trim());
      setNotionToken("");
      setNotionSaved(true);
    } catch (e) { setIntegrationError(String(e)); }
  }

  async function handleSaveGitHub() {
    if (!githubToken.trim()) return;
    setIntegrationError(null);
    try {
      await saveGitHubToken(githubToken.trim());
      setGithubToken("");
      setGithubSaved(true);
    } catch (e) { setIntegrationError(String(e)); }
  }

  async function handleSaveObsidian() {
    if (!obsidianKey.trim()) return;
    setIntegrationError(null);
    try {
      await saveObsidianKey(obsidianKey.trim());
      setObsidianKey("");
      setObsidianSaved(true);
    } catch (e) { setIntegrationError(String(e)); }
  }

  async function handleWakeToggle(enabled: boolean) {
    setWakeError(null);
    setWakeBusy(true);
    try {
      if (enabled) {
        await enableWakeWord();
      } else {
        await disableWakeWord();
      }
      await refreshWakeStatus();
    } catch (e) {
      setWakeError(String(e));
    } finally {
      setWakeBusy(false);
    }
  }

  async function handleWallpaperToggle(enabled: boolean) {
    setWallpaperBusy(true);
    setWallpaperError(null);
    try {
      if (enabled) {
        await enableWallpaper();
      } else {
        await disableWallpaper();
      }
      setWallpaperActive(enabled);
      await updateSetting("wallpaper_mode_enabled", String(enabled));
    } catch (e) {
      setWallpaperError(String(e));
    } finally {
      setWallpaperBusy(false);
    }
  }

  async function handleDownloadModel(force = false) {
    setWakeError(null);
    setWakeBusy(true);
    try {
      await downloadModel(force);
      await refreshWakeStatus();
    } catch (e) {
      setWakeError(String(e));
    } finally {
      setWakeBusy(false);
    }
  }

  // Mirrors VoiceEngine::is_available in src-tauri/src/voice/mod.rs: a Gemini
  // key (env or Keychain, which is what get_api_key_status reports) or the
  // downloaded local Whisper model. Without one of them push-to-talk errors out.
  function getVoiceAvailabilityLine(): string {
    const cloud = Boolean(apiKeyStatus["gemini"]);
    const local = Boolean(wakeStatus?.model_downloaded);
    if (cloud && local) return "Speech-to-text ready -- cloud Gemini, with the local Whisper model as fallback.";
    if (cloud) return "Speech-to-text ready -- cloud Gemini.";
    if (local) return "Speech-to-text ready -- local Whisper model.";
    return "Speech-to-text needs one of two things: a Gemini API key saved under API Keys, or the local Whisper model downloaded under Wake Word.";
  }

  function getWakeStatusLine(state: VoiceState): string {
    if (typeof state === "object" && "ModelDownloading" in state) {
      return `Downloading local model: ${state.ModelDownloading}%`;
    }
    if (typeof state === "object" && "Error" in state) {
      return `Error: ${state.Error}`;
    }
    if (state === "WakeWordListening") return "Wake word listening is active";
    if (state === "WakeWordDetected") return "Wake word detected";
    if (state === "WakeWordProcessing") return "Processing wake-word request";
    if (state === "WakeWordSpeaking") return "Speaking wake-word response";
    if (wakeStatus?.model_downloaded) return "Local speech model ready";
    return "Model required before wake-word listening can start";
  }

  if (!loaded) {
    return (
      <div style={{ padding: 24 }}>
        {loadError ? <>
          <p role="alert" className="settings-help">Could not load settings. Open the desktop app and try again.</p>
          <button style={styles.actionBtn} onClick={() => { void refreshSettings(); }}>Retry</button>
        </> : <p role="status" className="settings-help">Loading settings…</p>}
      </div>
    );
  }

  const options = [
    { value: "gemini_primary", label: "Gemini Flash (routine) + Gemini Pro (fallback)" },
    { value: "gemini_flash", label: "Gemini Flash (primary) + Pro (fallback)" },
    { value: "gemini_pro_only", label: "Gemini Pro only" },
    { value: "gemini_flash_only", label: "Gemini Flash only" },
  ];

  const modelDownloading =
    typeof voiceState === "object" && "ModelDownloading" in voiceState
      ? voiceState.ModelDownloading
      : null;
  const wakeCanEnable = Boolean(wakeStatus?.model_downloaded || wakeStatus?.enabled);
  const voiceInputReady = Boolean(apiKeyStatus["gemini"] || wakeStatus?.model_downloaded);

  const SECTIONS = [
    { id: "ai", label: "Models", Icon: () => <Cpu size={17} /> },
    { id: "keys", label: "API keys", Icon: () => <Key size={17} /> },
    { id: "google", label: "Assistant Google", Icon: () => <Globe size={17} /> },
    { id: "notion", label: "Notion", Icon: () => <NotionMark size={17} /> },
    { id: "github", label: "GitHub", Icon: () => <GithubMark size={17} /> },
    { id: "obsidian", label: "Obsidian", Icon: () => <Gem size={17} /> },
    { id: "voice", label: "Voice", Icon: () => <Mic size={17} /> },
    { id: "assistant", label: "Assistant", Icon: () => <Bot size={17} /> },
    { id: "appearance", label: "Appearance", Icon: () => <Palette size={17} /> },
  ];

  return (
    <div className="settings-workspace">
      <nav className="settings-nav" aria-label="Settings categories">
        <div className="settings-nav-title">Settings</div>
        {SECTIONS.map((s) => {
          const active = section === s.id;
          return (
            <button key={s.id} onClick={() => setSection(s.id)}
              className={`settings-nav-item${active ? " is-active" : ""}`}
              aria-current={active ? "page" : undefined}>
              <s.Icon /> <span>{s.label}</span>
            </button>
          );
        })}
      </nav>
      <div className="settings-content">
        <header className="settings-page-header">
          <h1>{SECTIONS.find((item) => item.id === section)?.label ?? "Settings"}</h1>
          <p>Manage how JARVIS works on this Mac.</p>
        </header>
        {integrationError && ["notion", "github", "obsidian"].includes(section) && <p role="alert" style={styles.errorText}>{integrationError}</p>}
        {section === "ai" && (<>
          <div className="settings-info-row">
            <p>These providers handle Assistant conversations. Mail has its own analysis model and uses the API keys saved here.</p>
            <button style={styles.saveBtn} onClick={() => open("email")}>Mail analysis settings</button>
          </div>
          {llmError && <p role="alert" style={styles.errorText}>{llmError}</p>}
          <div className="settings-panel">
            <h2 className="settings-heading">Provider priority</h2>
            <div style={styles.hint}>JARVIS tries enabled providers in this order. Use the arrows to change priority.</div>

            {chain.map((entry, i) => {
              const label = entry.provider_type === "local"
                ? `${entry.model_id || "unknown"} (local)`
                : entry.provider_type === "gemini"
                ? `Gemini (${entry.model_id || "default"})`
                : entry.provider_type === "claude"
                ? `Claude (${entry.model_id || "default"})`
                : entry.provider_type === "openai"
                ? `OpenAI (${entry.model_id || "default"})`
                : entry.provider_type === "bedrock"
                ? `Bedrock (${entry.model_id || "default"})`
                : entry.provider_type;
              return (
                <div key={i} className="settings-chain-row">
                  <span style={{ color: "var(--text-muted)", fontSize: 12, fontFamily: "var(--font-mono)", width: 16 }}>{i + 1}.</span>
                  <span style={{ color: entry.enabled ? "var(--text-primary)" : "var(--text-secondary)", fontSize: 13, minWidth: 0, overflowWrap: "anywhere" }}>{label}</span>
                  <button aria-label={`Move ${label} up`} onClick={() => handleMoveChainEntry(i, "up")} disabled={i === 0} style={{ ...styles.saveBtn, fontSize: 12, padding: "1px 5px" }}>↑</button>
                  <button aria-label={`Move ${label} down`} onClick={() => handleMoveChainEntry(i, "down")} disabled={i === chain.length - 1} style={{ ...styles.saveBtn, fontSize: 12, padding: "1px 5px" }}>↓</button>
                  <label style={{ cursor: "pointer" }}>
                    <input type="checkbox" aria-label={`Enable ${label}`} checked={entry.enabled} onChange={() => handleToggleChainEntry(i)} style={styles.radio} />
                  </label>
                  <button aria-label={`Remove ${label} from priority`} onClick={() => handleRemoveFromChain(i)} style={{ ...styles.saveBtn, fontSize: 12, padding: "1px 5px", borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}>Remove</button>
                </div>
              );
            })}

            {chain.length === 0 && (
              <div style={{ color: "var(--text-muted)", fontSize: 13, marginBottom: 8 }}>No providers configured. Add cloud or local providers below.</div>
            )}

            <div style={{ display: "flex", gap: 6, marginTop: 8, flexWrap: "wrap" }}>
              <button onClick={() => handleAddToChain("gemini", undefined, GEMINI_MODEL_PRO)} style={styles.saveBtn}>+ Gemini Pro</button>
              <button onClick={() => handleAddToChain("gemini", undefined, GEMINI_MODEL_FLASH)} style={styles.saveBtn}>+ Gemini Flash</button>
              <button onClick={() => handleAddToChain("claude")} style={styles.saveBtn}>+ Claude</button>
              <button onClick={() => handleAddToChain("openai")} style={styles.saveBtn}>+ OpenAI</button>
              <button onClick={() => handleAddToChain("bedrock", undefined, "anthropic.claude-sonnet-4-6")} style={styles.saveBtn}>+ Bedrock Claude</button>
            </div>
          </div>

          <div className="settings-panel">
            <h2 className="settings-heading">Default provider</h2>
            <div style={styles.hint}>Used when no provider chain is configured</div>
            {options.map((opt) => (
              <label key={opt.value} style={styles.option}>
                <input type="radio" name="ai_provider" value={opt.value} checked={aiProvider === opt.value} onChange={() => handleProviderChange(opt.value)} style={styles.radio} />
                <span style={styles.optionLabel}>{opt.label}</span>
              </label>
            ))}
          </div>
          <details className="settings-local-details">
            <summary>Local AI setup and endpoints</summary>
            <p className="settings-help">Optional. Connect a local server or download a model only when you want to use local AI.</p>
            <div className="settings-local-setup">
              <LocalAiSetup
                endpoints={endpoints}
                onEndpointAdded={(ep) => setEndpoints((prev) => [...prev, ep])}
                onModelsLoaded={(eid, models) => setEndpointModels((prev) => ({ ...prev, [eid]: models }))}
                onAddToChain={handleAddToChain}
              />
            </div>
          <div className="settings-panel">
            <h2 className="settings-heading">Local model endpoints</h2>
            <div style={styles.hint}>Connect to Ollama, vLLM, or any compatible server</div>

            {endpoints.map((ep) => (
              <div key={ep.id} style={{ background: "var(--surface)", border: "1px solid var(--border-primary)", borderRadius: 6, padding: 10, marginBottom: 8 }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  <div>
                    <span style={{ color: "var(--text-primary)", fontSize: 12, fontWeight: 600 }}>{ep.name}</span>
                    <span style={{ color: "var(--text-muted)", fontSize: 12, marginLeft: 8 }}>{ep.backend_type.toUpperCase()}</span>
                  </div>
                  <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
                    <span style={{ width: 8, height: 8, borderRadius: "50%", background: ep.last_health_status ? "var(--text-secondary)" : "var(--accent-urgent)" }} />
                  </div>
                </div>
                <div style={{ color: "var(--text-muted)", fontSize: 12, fontFamily: "var(--font-mono)", marginTop: 4 }}>{ep.url}</div>
                {endpointHealth[ep.id] && (
                  <div style={{ color: "var(--text-secondary)", fontSize: 12, marginTop: 4 }}>
                    {endpointHealth[ep.id].reachable ? `${endpointHealth[ep.id].model_count} models, ${endpointHealth[ep.id].latency_ms}ms` : "Unreachable"}
                  </div>
                )}
                {endpointModels[ep.id] && endpointModels[ep.id].length > 0 && (
                  <div style={{ marginTop: 6 }}>
                    <div style={{ color: "var(--text-muted)", fontSize: 12 }}>Available models</div>
                    {endpointModels[ep.id].map((m) => (
                      <div key={m.id} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginTop: 2 }}>
                        <span style={{ color: "var(--text-secondary)", fontSize: 12, fontFamily: "var(--font-mono)" }}>{m.id}</span>
                        <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
                          <span style={{ color: "var(--text-muted)", fontSize: 12 }}>{m.supports_tools === "native" ? "tools" : m.supports_tools === "prompt_injected" ? "prompt-tools" : "chat"}</span>
                          <button onClick={() => handleAddToChain("local", ep.id, m.id)} style={{ ...styles.saveBtn, fontSize: 12, padding: "2px 6px" }}>Add to priority</button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
                <div style={{ display: "flex", gap: 6, marginTop: 8 }}>
                  <button onClick={() => handleTestEndpoint(ep.id)} style={{ ...styles.saveBtn, fontSize: 12 }}>Test connection</button>
                  <button onClick={() => handleRemoveEndpoint(ep.id)} style={{ ...styles.saveBtn, fontSize: 12, borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}>Remove</button>
                </div>
              </div>
            ))}

            <div style={{ display: "flex", gap: 6, marginTop: 8, flexDirection: "column" as const }}>
              <input aria-label="Endpoint name" value={newEndpointName} onChange={(e) => setNewEndpointName(e.target.value)} placeholder="Name (optional)" style={styles.tokenInput} />
              <div style={{ display: "flex", gap: 6 }}>
                <input aria-label="Endpoint URL" value={newEndpointUrl} onChange={(e) => setNewEndpointUrl(e.target.value)} placeholder="http://localhost:11434" style={{ ...styles.tokenInput, flex: 1 }} />
                <button onClick={handleAddEndpoint} disabled={endpointBusy} style={styles.saveBtn}>{endpointBusy ? "..." : "Add endpoint"}</button>
              </div>
            </div>
          </div>
          </details>
        </>)}

        {section === "keys" && (
          <div className="settings-panel">
            <h2 className="settings-heading">Provider API keys</h2>
            <div style={styles.hint}>Keys are stored in macOS Keychain and used to authenticate requests to the selected provider.</div>
            {API_KEY_PROVIDERS.map(({ id, label, help }) => (
              <div key={id} style={{ marginBottom: 12 }}>
                <div style={styles.hint}>
                  {label}
                  {apiKeyStatus[id] ? <span style={{ color: "var(--text-secondary)" }}> · Key available</span> : null}
                </div>
                <div style={{ display: "flex", gap: 8 }}>
                  <input
                    type="password"
                    aria-label={`${label} API key`}
                    autoComplete="off"
                    style={styles.tokenInput}
                    placeholder={apiKeyStatus[id] ? "Replace saved key" : help}
                    value={apiKeyDrafts[id] ?? ""}
                    onChange={(e) => setApiKeyDrafts((d) => ({ ...d, [id]: e.target.value }))}
                  />
                  <button
                    onClick={() => { void handleSaveApiKey(id); }}
                    disabled={!(apiKeyDrafts[id] ?? "").trim()}
                    style={styles.saveBtn}
                  >Save</button>
                  {apiKeyStatus[id] && (
                    <button
                      onClick={() => { void handleClearApiKey(id, label); }}
                      style={{ ...styles.saveBtn, borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}
                    >Remove key</button>
                  )}
                </div>
              </div>
            ))}
            <div style={{ ...styles.hint, marginTop: 4, marginBottom: 0 }}>
              Saved keys are never displayed. Leave a field blank to keep its current key,
              or enter a new key to replace it. Remove key deletes the saved key.
            </div>
            {apiKeyError && <div style={styles.errorText}>{apiKeyError}</div>}
          </div>
        )}

        {section === "google" && (
          <div className="settings-panel">
            <h2 className="settings-heading">Google for Assistant and Calendar</h2>
            <div style={styles.hint}>Connect Google for Assistant mail tools, Calendar and briefings. Mail workspaces use a separate Gmail connection.</div>
            {googleConnected === null ? (
              <div style={styles.hint}>
                {googleError ? "Google connection status is unavailable." : "Checking Google connection…"}
                {googleError && <button style={{ ...styles.saveBtn, marginLeft: 10 }} onClick={() => { setGoogleError(null); void refreshGoogle(); }}>Retry</button>}
              </div>
            ) : googleConnected ? (
              <>
                <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 10 }}>
                  {googleAccount ? `Connected as ${googleAccount}` : "Connected"}
                </div>
                <div style={{ display: "flex", gap: 8 }}>
                  <button
                    onClick={() => { void handleGoogleConnect(true); }}
                    disabled={connecting || disconnecting}
                    style={styles.actionBtn}
                  >
                    {connecting ? "Opening…" : "Switch account"}
                  </button>
                  <button
                    onClick={() => { void handleGoogleDisconnect(); }}
                    disabled={connecting || disconnecting}
                    style={{ ...styles.actionBtn, borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}
                  >
                    {disconnecting ? "Disconnecting…" : "Disconnect"}
                  </button>
                </div>
                <div style={{ ...styles.hint, marginTop: 8, marginBottom: 0 }}>
                  Disconnecting revokes this Google connection and removes its Assistant mail cache,
                  Calendar events and briefings. Local copies in the separate Mail workspace are kept.
                </div>
              </>
            ) : (
              <button onClick={() => { void handleGoogleConnect(false); }} disabled={connecting} style={styles.actionBtn}>
                {connecting ? "Connecting…" : "Connect Google"}
              </button>
            )}
            {googleNotice && <div style={{ ...styles.hint, marginTop: 10, marginBottom: 0 }}>{googleNotice}</div>}
            {googleError && <div style={styles.errorText}>{googleError}</div>}
          </div>
        )}

        {section === "notion" && (
          <div className="settings-panel">
            <h2 className="settings-heading">Notion</h2>
            <div style={styles.hint}>Enter your Notion integration token</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" aria-label="Notion integration token" autoComplete="off" value={notionToken} onChange={(e) => { setNotionToken(e.target.value); setNotionSaved(false); }} placeholder="ntn_..." style={styles.tokenInput} />
              <button disabled={!notionToken.trim()} onClick={handleSaveNotion} style={styles.saveBtn}>{notionSaved ? "Saved" : "Save"}</button>
            </div>
          </div>
        )}

        {section === "github" && (
          <div className="settings-panel">
            <h2 className="settings-heading">GitHub</h2>
            <div style={styles.hint}>Enter your GitHub personal access token</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" aria-label="GitHub personal access token" autoComplete="off" value={githubToken} onChange={(e) => { setGithubToken(e.target.value); setGithubSaved(false); }} placeholder="ghp_..." style={styles.tokenInput} />
              <button disabled={!githubToken.trim()} onClick={handleSaveGitHub} style={styles.saveBtn}>{githubSaved ? "Saved" : "Save"}</button>
            </div>
          </div>
        )}

        {section === "obsidian" && (
          <div className="settings-panel">
            <h2 className="settings-heading">Obsidian</h2>
            <div style={styles.hint}>Connect to Obsidian vault via Local REST API plugin</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" aria-label="Obsidian API key" autoComplete="off" value={obsidianKey} onChange={(e) => { setObsidianKey(e.target.value); setObsidianSaved(false); }} placeholder="API key from Obsidian REST API plugin" style={styles.tokenInput} />
              <button disabled={!obsidianKey.trim()} onClick={handleSaveObsidian} style={styles.saveBtn}>{obsidianSaved ? "Saved" : "Save"}</button>
            </div>
            <div style={{ color: "var(--text-muted)", fontSize: 12, marginTop: 6 }}>
              Install "Local REST API" plugin in Obsidian, enable it, copy the API key
            </div>
            <div style={{ marginTop: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>Endpoint</div>
              <input
                aria-label="Obsidian endpoint URL"
                value={obsidianBaseUrl}
                onChange={(e) => setObsidianBaseUrl(e.target.value)}
                onBlur={() => { void setPreference("obsidian_base_url", obsidianBaseUrl.trim()); }}
                placeholder="https://127.0.0.1:27124"
                style={styles.tokenInput}
              />
              <div style={{ color: "var(--text-muted)", fontSize: 12, marginTop: 6 }}>
                Leave blank for the plugin default (HTTPS on 27124). Set this only if you changed
                the port, or enabled the plugin's non-encrypted server on http://127.0.0.1:27123.
              </div>
            </div>
          </div>
        )}

        {section === "ai" && (
          <div className="settings-panel">
            <h2 className="settings-heading">AWS Bedrock</h2>
            <div style={styles.hint}>
              Used for Claude (Haiku 4.5 / Sonnet 4.6) and Amazon Nova models via the
              apac. inference profile (Singapore region).
              {awsStatus.configured && (
                <> Currently configured for <code style={{ fontFamily: "var(--font-mono)", color: "var(--text-primary)" }}>{awsStatus.region}</code>.</>
              )}
            </div>
            <div style={{ marginBottom: 8 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>Access Key ID</div>
              <input aria-label="AWS access key ID" value={awsAccessKey} onChange={(e) => setAwsAccessKey(e.target.value)} placeholder="AKIA..." style={styles.tokenInput} />
            </div>
            <div style={{ marginBottom: 8 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>Secret Access Key</div>
              <input type="password" aria-label="AWS secret access key" autoComplete="off" value={awsSecret} onChange={(e) => setAwsSecret(e.target.value)} placeholder="Secret key" style={styles.tokenInput} />
            </div>
            <div style={{ marginBottom: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>Region</div>
              <select aria-label="AWS region" value={awsRegion} onChange={(e) => setAwsRegion(e.target.value)} style={styles.select}>
                <option value="ap-southeast-1">Singapore (ap-southeast-1) -- recommended</option>
                <option value="ap-northeast-1">Tokyo (ap-northeast-1) -- fallback</option>
                <option value="us-east-1">US East (us-east-1) -- high latency from HK</option>
              </select>
            </div>
            <div style={{ display: "flex", gap: 8 }}>
              <button disabled={!awsAccessKey.trim() || !awsSecret.trim()} onClick={async () => {
                setAwsError(null);
                try {
                  await setAwsCredentials(awsAccessKey, awsSecret, awsRegion);
                  setAwsAccessKey(""); setAwsSecret("");
                  setAwsStatus(await getAwsCredentialsStatus());
                } catch (e) {
                  setAwsError(String(e));
                }
              }} style={styles.saveBtn}>Save</button>
              {awsStatus.configured && (
                <button onClick={async () => {
                  await clearAwsCredentials();
                  setAwsStatus({ configured: false, region: null });
                }} style={{ ...styles.saveBtn, borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}>Remove key</button>
              )}
            </div>
            {awsError && <div style={styles.errorText}>{awsError}</div>}
          </div>
        )}

        {section === "voice" && (<>
          <div className="settings-panel">
            <h2 className="settings-heading">Language</h2>
            <div style={styles.hint}>
              Controls the assistant's response language and speech-to-text language.
            </div>

            <div style={{ marginTop: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>Response language</div>
              <select aria-label="Response language" value={language} style={styles.select} onChange={async (e) => {
                const v = e.target.value;
                setLanguage(v);
                await setPreference("language", v);
              }}>
                <option value="en">English</option>
                <option value="yue">廣東話 (Cantonese)</option>
                <option value="bilingual">Bilingual (code-mix)</option>
              </select>
            </div>

            <div style={{ marginTop: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>Speech-to-text language</div>
              <select aria-label="Speech-to-text language" value={sttLanguage} style={styles.select} onChange={async (e) => {
                const v = e.target.value;
                setSttLanguage(v);
                await setPreference("stt_language", v);
              }}>
                <option value="auto">Auto-detect</option>
                <option value="en">English</option>
                <option value="yue">廣東話</option>
              </select>
            </div>
          </div>

          <div className="settings-panel">
            <h2 className="settings-heading">Speech</h2>
            <div style={styles.hint}>Cmd+Shift+J to start/stop voice input</div>
            <div style={voiceInputReady ? styles.hint : styles.statusText}>{getVoiceAvailabilityLine()}</div>
            <label style={styles.option}>
              <input type="checkbox" checked={voiceSettings?.tts_enabled ?? true} onChange={(e) => { void setVoiceSetting("tts_enabled", String(e.target.checked)); setVoiceSettingsState((prev) => prev ? { ...prev, tts_enabled: e.target.checked } : prev); }} style={styles.radio} />
              <span style={styles.optionLabel}>Enable text-to-speech</span>
            </label>
            {ttsVoices.length > 0 && (
              <div style={{ marginTop: 8 }}>
                <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>TTS Voice</div>
                <select aria-label="Speech voice" value={voiceSettings?.tts_voice ?? "Samantha"} onChange={(e) => { void setVoiceSetting("tts_voice", e.target.value); setVoiceSettingsState((prev) => prev ? { ...prev, tts_voice: e.target.value } : prev); }} style={styles.select}>
                  {ttsVoices.map((voice) => <option key={voice} value={voice}>{voice}</option>)}
                </select>
              </div>
            )}
            {ttsVoices.length > 0 && (
              <div style={{ marginTop: 8 }}>
                <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>Cantonese voice</div>
                <select aria-label="Cantonese voice" value={ttsVoiceYue} onChange={(e) => { void setVoiceSetting("tts_voice_yue", e.target.value); setTtsVoiceYue(e.target.value); }} style={styles.select}>
                  {ttsVoices.map((voice) => <option key={voice} value={voice}>{voice}</option>)}
                </select>
              </div>
            )}
          </div>

          <div className="settings-panel">
            <h2 className="settings-heading">Wake word</h2>
            <div style={styles.hint}>Experimental. Listen locally for "Hey Jarvis" to start voice input.</div>
            <label style={styles.option}>
              <input type="checkbox" checked={wakeStatus?.enabled ?? false} disabled={!wakeCanEnable || wakeBusy || modelDownloading !== null} onChange={(e) => { void handleWakeToggle(e.target.checked); }} style={styles.radio} />
              <span style={styles.optionLabel}>Enable wake word</span>
            </label>
            <div style={styles.statusText}>{getWakeStatusLine(voiceState)}</div>
            <div style={{ display: "flex", gap: 8 }}>
              <button onClick={() => { void handleDownloadModel(false); }} disabled={wakeBusy || modelDownloading !== null || wakeStatus?.model_downloaded} style={styles.actionBtn}>
                {modelDownloading !== null ? "Downloading…" : wakeStatus?.model_downloaded ? "Model ready" : "Download model"}
              </button>
              {wakeStatus?.model_downloaded && (
                <button onClick={() => { void handleDownloadModel(true); }} disabled={wakeBusy || modelDownloading !== null} style={styles.actionBtn}>
                  Download again
                </button>
              )}
            </div>
            {modelDownloading !== null && (
              <div style={styles.progressShell}>
                <div style={{ ...styles.progressFill, width: `${modelDownloading}%` }} />
              </div>
            )}
            {wakeStatus?.model_downloaded && modelDownloading === null && (
              <div style={{ ...styles.hint, marginTop: 8 }}>
                Model file: ~/Library/Application Support/jarvis/models/ggml-base.bin. If transcription keeps
                failing to load the model, that file is damaged: Download again replaces it in place.
              </div>
            )}
            <div style={styles.privacyText}>
              Wake-word audio is processed locally for detection. After activation, full commands use cloud speech-to-text first when a Gemini API key is saved under API Keys, then fall back to local Whisper.
            </div>
            {wakeError && <div style={styles.errorText}>{wakeError}</div>}
          </div>
        </>)}

        {section === "assistant" && (
          <div className="settings-panel">
            <h2 className="settings-heading">Briefing</h2>
            <label style={styles.option}>
              <input type="checkbox" checked={autoBriefing} onChange={(e) => { setAutoBriefing(e.target.checked); void updateSetting("auto_briefing", String(e.target.checked)); }} style={styles.radio} />
              <span style={styles.optionLabel}>Speak morning briefing on startup (once per day)</span>
            </label>
          </div>
        )}

        {section === "appearance" && (<>
          <div className="settings-panel">
            <h2 className="settings-heading">Colour theme</h2>
            <p style={styles.hint}>Choose a light or dark workspace, or follow your Mac.</p>
            <div className="settings-appearance-options" role="group" aria-label="Colour theme">
              {([
                { value: "light", label: "Light", help: "A bright workspace" },
                { value: "dark", label: "Dark", help: "A darker workspace" },
                { value: "system", label: "System", help: "Follow macOS appearance" },
              ] as const).map((mode) => (
                <button key={mode.value} className={appearance === mode.value ? "is-selected" : ""}
                  aria-pressed={appearance === mode.value} onClick={() => setAppearance(mode.value)}>
                  <strong>{mode.label}</strong><span>{mode.help}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="settings-panel">
            <h2 className="settings-heading">Desktop wallpaper</h2>
            <div style={styles.hint}>
              Turn the full JARVIS app into a live macOS desktop wallpaper.
              The entire UI renders behind all windows and desktop icons.
            </div>
            <label style={styles.option}>
              <input type="checkbox" checked={wallpaperActive} disabled={wallpaperBusy} onChange={(e) => { void handleWallpaperToggle(e.target.checked); }} style={styles.radio} />
              <span style={styles.optionLabel}>Enable wallpaper mode</span>
            </label>
            <div style={styles.statusText}>
              {wallpaperBusy ? "Toggling..." : wallpaperActive ? "Wallpaper active -- full JARVIS on desktop" : "Wallpaper inactive"}
            </div>
            {wallpaperError && <p role="alert" style={styles.errorText}>{wallpaperError}</p>}
            <div style={styles.privacyText}>
              When active, JARVIS goes fullscreen behind all windows.
              After enabling, it stays interactive until you send it to the background.
              To bring it back later, say "Hey Jarvis", click the tray icon, or press Cmd+Shift+W.
              Closing the window sends it back to the background.
            </div>
          </div>
        </>)}

      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  option: { display: "flex", alignItems: "center", gap: 10, marginBottom: 12, cursor: "pointer", lineHeight: 1.5 },
  radio: { accentColor: "var(--cyan)", width: 16, height: 16, flexShrink: 0 },
  optionLabel: { color: "var(--text-primary)", fontSize: 13 },
  hint: { color: "var(--text-secondary)", fontSize: 13, marginBottom: 12, lineHeight: 1.6, maxWidth: "72ch" },
  tokenInput: { flex: 1, minWidth: 0, width: "100%", background: "var(--bg-primary)", border: "1px solid var(--border-primary)", borderRadius: 6, padding: "9px 11px", color: "var(--text-primary)", fontSize: 13, fontFamily: "inherit" },
  saveBtn: { background: "var(--surface-solid)", border: "1px solid var(--border-primary)", borderRadius: 6, padding: "8px 12px", color: "var(--text-primary)", cursor: "pointer", fontFamily: "inherit", fontSize: 13, whiteSpace: "nowrap" },
  actionBtn: { background: "var(--surface-solid)", border: "1px solid var(--border-primary)", borderRadius: 6, padding: "9px 14px", color: "var(--text-primary)", cursor: "pointer", fontFamily: "inherit", fontSize: 13 },
  select: { maxWidth: "100%", background: "var(--bg-primary)", border: "1px solid var(--border-primary)", borderRadius: 6, padding: "8px 10px", color: "var(--text-primary)", fontSize: 13, fontFamily: "inherit" },
  statusText: { color: "var(--text-secondary)", fontSize: 13, marginBottom: 12, lineHeight: 1.6 },
  progressShell: { marginTop: 12, width: "100%", height: 6, borderRadius: 4, overflow: "hidden", background: "var(--border-primary)" },
  progressFill: { height: "100%", background: "var(--cyan)", transition: "width 180ms ease-out" },
  privacyText: { color: "var(--text-secondary)", fontSize: 13, marginTop: 12, lineHeight: 1.6, maxWidth: "72ch" },
  errorText: { color: "var(--accent-urgent)", fontSize: 13, marginTop: 12, lineHeight: 1.5 },
};
