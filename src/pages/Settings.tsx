import { useEffect, useRef, useState } from "react";
import {
  accountStatus,
  getSettings,
  updateSetting,
  googleConnect,
  googleCancelConnect,
  googleAccounts,
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
  getStorageUsage,
  runStorageSweep,
  purgeStorage,
  setRetentionPolicy,
} from "../lib/commands";
import type { GoogleAccounts } from "../lib/commands";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { AccountStatus, VoiceSettings, WakeWordStatus, VoiceState, LocalEndpoint, LocalModel, ProviderChainEntry, EndpointHealth, StorageUsage, StorageScope } from "../lib/types";
import { GOOGLE_ALREADY_CONNECTED } from "../lib/types";
import { useVoiceState } from "../hooks/useVoiceState";
import { useUiLocale } from "../i18n";
import IntegrationConnection from "../components/IntegrationConnection";
import OtherGoogleAccounts from "../components/OtherGoogleAccounts";
import { connectionStatuses, connectionsChanged } from "../lib/connections";
import { DISPLAY_NAME_KEY, accountSectionVisible, displayNameChanged } from "../lib/account";
import LocalAiSetup from "../components/LocalAiSetup";
import ChooseAi from "../components/ChooseAi";
import { useAiModels } from "../lib/ai-models";
import { aiServiceLabel, currentAi, type CurrentAi } from "../lib/ai-choice";
import AccountPanel from "../components/AccountPanel";
import { UserRound } from "lucide-react";
import { useWorkspace, useWorkspaceValue } from "../workspace/WorkspaceContext";
import "./settings-workspace.css";
import { MAX_ADDRESS_CHARS, MAX_INSTRUCTIONS_CHARS, PERSONA_CHOICES, personaLabel, personaValue, type PersonaKey } from "../lib/persona";
import { canSwitchListening, voiceStateKind } from "../lib/always-listening";
import { Cpu, Key, Globe, Gem, Mic, Bot, Palette, HardDrive, GithubMark, NotionMark } from "../components/icons";

const API_KEY_PROVIDERS = [
  { id: "gemini", label: "Google Gemini", help: "Gemini chat and speech-to-text" },
  { id: "claude", label: "Anthropic Claude", help: "Claude chat and fallback" },
  { id: "openai", label: "OpenAI", help: "OpenAI chat and Mail analysis" },
] as const;

// Deleting a saved key is its own IPC call: set_api_key refuses a blank value
// unless `remove` is set, so an accidental SAVE on the (permanently blank) key
// field can no longer destroy a working credential. src/lib/commands.ts has no
// wrapper for the removal form yet, hence the direct invoke -- the arg names
// are single lowercase words, so its camelCase normalisation is a no-op here.
function clearApiKey(provider: string): Promise<void> {
  return invoke("set_api_key", { provider, value: "", remove: true });
}

export default function Settings() {
  // Model ids come from src-tauri/src/ai/models.rs, never from this file.
  const models = useAiModels();
  const { appearance, setAppearance, open } = useWorkspace();
  const { locale, setLocale, saveError: localeSaveError, retrySave: retryLocaleSave, t, text } = useUiLocale();
  const [aiProvider, setAiProvider] = useState("gemini_primary");
  const [autoBriefing, setAutoBriefing] = useState(false);
  const [displayName, setDisplayName] = useState("");
  const [persona, setPersona] = useState<Record<string, string>>({});
  const [personaText, setPersonaText] = useState({ assistant_address: "", assistant_instructions: "" });
  const [personaSaved, setPersonaSaved] = useState(false);
  const [personaError, setPersonaError] = useState<string | null>(null);
  const [displayNameSaved, setDisplayNameSaved] = useState(false);
  const [apiKeyDrafts, setApiKeyDrafts] = useState<Record<string, string>>({});
  const [apiKeyStatus, setApiKeyStatus] = useState<Record<string, boolean>>({});
  const [apiKeyError, setApiKeyError] = useState<string | null>(null);
  // Whether accounts are available in this build -- unknown until this loads,
  // and unknown reads as unavailable so the Account section never flashes in
  // and out. Loaded once; availability does not change while Settings is open.
  const [accountInfo, setAccountInfo] = useState<AccountStatus | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [google, setGoogle] = useState<GoogleAccounts | null>(null);
  const [connecting, setConnecting] = useState(false);
  const googleCancelled = useRef(false);
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
  // Gemini hears speech only when it is the chosen AI (voice/mod.rs).
  const [chosenAi, setChosenAi] = useState<CurrentAi | null>(null);
  useEffect(() => { void currentAi().then(setChosenAi).catch(() => {}); }, []);
  const [wakeBusy, setWakeBusy] = useState(false);
  const [wakeError, setWakeError] = useState<string | null>(null);
  const { state: voiceState } = useVoiceState();
  const [wallpaperActive, setWallpaperActive] = useState(false);
  const [wallpaperBusy, setWallpaperBusy] = useState(false);
  const [wallpaperError, setWallpaperError] = useState<string | null>(null);

  useEffect(() => {
    getApiKeyStatus().then(setApiKeyStatus).catch(() => {});
  }, []);

  useEffect(() => {
    accountStatus().then(setAccountInfo).catch(() => {});
  }, []);

  // Check the three services once, so moving between their pages shows a
  // status at once instead of waiting on a new check each time.
  useEffect(() => {
    for (const service of ["notion", "github", "obsidian"] as const) {
      void connectionStatuses.load(service).catch(() => {});
    }
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
    if (!window.confirm(`${text("Remove the saved", "是否移除已儲存的")} ${label} ${text("key from your Keychain? This cannot be undone.", "密鑰？此操作無法復原。")}`)) {
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

  // A previous session can have left the workspace on "account"; once we know
  // that section is hidden in this build, land on the section right after it
  // instead of a nav page nothing renders for.
  useEffect(() => {
    if (section === "account" && !accountSectionVisible(accountInfo)) {
      setSection("ai");
    }
  }, [accountInfo, section, setSection]);

  // Storage state
  const [storage, setStorage] = useState<StorageUsage | null>(null);
  const [storageBusy, setStorageBusy] = useState("");
  const [storageNotice, setStorageNotice] = useState("");
  const [storageError, setStorageError] = useState("");

  async function refreshSettings() {
    setLoadError(false);
    try {
      const s = await getSettings();
      setAiProvider(s.values["ai_provider"] || "gemini_primary");
      // Same rule as the startup reader in lib.rs: an absent row means off.
      setAutoBriefing(s.values["auto_briefing"] === "true");
      setDisplayName(s.values[DISPLAY_NAME_KEY] ?? "");
      setPersona(Object.fromEntries(Object.keys(PERSONA_CHOICES).map(key => [key, personaValue(key as PersonaKey, s.values[key] ?? null)])));
      setPersonaText({ assistant_address: s.values.assistant_address ?? "", assistant_instructions: s.values.assistant_instructions ?? "" });
      setLoaded(true);
    } catch {
      setLoadError(true);
    }
  }

  // Persist on blur and on the button: the sidebar reads the saved row, so an
  // unsaved edit must never be what the row shows.
  async function saveDisplayName() {
    try {
      await updateSetting(DISPLAY_NAME_KEY, displayName.trim());
      setDisplayNameSaved(true);
      displayNameChanged();
    } catch {
      setDisplayNameSaved(false);
    }
  }

  useEffect(() => { void refreshSettings(); }, []);

  useEffect(() => {
    void refreshGoogle();
  }, []);

  useEffect(() => {
    getVoiceSettings().then(setVoiceSettingsState).catch(() => setWakeError("Could not load voice settings."));
    listTtsVoices().then(setTtsVoices).catch(() => setWakeError("Could not load the available voices."));
  }, []);

  // This tab stays mounted while hidden, and the home switch or a download can
  // change listening meanwhile: re-read whenever the voice state changes kind.
  const voiceKind = voiceStateKind(voiceState);
  useEffect(() => { void refreshWakeStatus(); }, [voiceKind]);

  useEffect(() => {
    listLocalEndpoints().then(setEndpoints).catch(() => {});
    getProviderChain().then(setChain).catch(() => {});
  }, []);

  // Reading the storage report walks the models and cache directories, so it is
  // deferred until the panel is actually opened rather than run on every mount.
  useEffect(() => {
    if (section !== "storage" || storage) return;
    void refreshStorage();
  }, [section]);

  async function refreshStorage() {
    try {
      setStorage(await getStorageUsage());
      setStorageError("");
    } catch (reason) {
      setStorageError(text("Could not read local storage usage.", "未能讀取本機儲存用量。"));
      void reason;
    }
  }

  async function runStorageAction(label: string, action: () => Promise<string>) {
    if (storageBusy) return;
    setStorageBusy(label); setStorageError(""); setStorageNotice("");
    try {
      setStorageNotice(await action());
      await refreshStorage();
    } catch (reason) {
      setStorageError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setStorageBusy("");
    }
  }

  function handleSweep() {
    void runStorageAction(text("Sweeping", "正在清理"), async () => {
      const report = await runStorageSweep();
      const rows = report.mailBodiesCleared + report.conversationsDeleted
        + report.emailsDeleted + report.metricsDeleted + report.briefingsDeleted;
      return rows === 0
        ? text("Nothing was old enough to remove.", "沒有超過保留期限的資料。")
        : text(
            `Removed ${rows} item(s): ${report.mailBodiesCleared} mail bodies, ${report.conversationsDeleted} chat turns, ${report.emailsDeleted} cached emails.`,
            `已清理 ${rows} 項：郵件內文 ${report.mailBodiesCleared}、對話記錄 ${report.conversationsDeleted}、快取郵件 ${report.emailsDeleted}。`,
          );
    });
  }

  function handlePurge(scope: StorageScope, label: string) {
    void runStorageAction(label, async () => {
      const freed = await purgeStorage(scope);
      return scope === "conversations"
        ? text(`Deleted ${freed} chat turn(s).`, `已刪除 ${freed} 項對話記錄。`)
        : text(`Freed ${formatBytes(freed)}.`, `已釋放 ${formatBytes(freed)}。`);
    });
  }

  function handleRetentionChange(days: number, enabled: boolean) {
    void runStorageAction(text("Saving", "正在儲存"), async () => {
      await setRetentionPolicy(days, enabled);
      return enabled
        ? text(`Retention set to ${days} days.`, `保留期限已設為 ${days} 日。`)
        : text("Retention switched off. JARVIS will keep everything.", "已關閉自動清理，JARVIS 會保留所有資料。");
    });
  }

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
  // The main account's label is the Google profile name and address saved at
  // sign-in; disconnect deletes both.
  async function refreshGoogle() {
    try {
      setGoogle(await googleAccounts());
    } catch (e) {
      setGoogleError(String(e));
      setGoogle(null);
    }
  }

  const mainLabel = google?.main.name && google.main.email
    ? `${google.main.name}（${google.main.email}）`
    : google?.main.name ?? google?.main.email ?? null;

  async function handleGoogleConnect(force: boolean) {
    // Switching revokes the current grant and wipes the cached mail before the
    // new flow starts, so it gets the same confirm-then-act treatment as CLEAR.
    if (force && !window.confirm(
      text("Switch the main Google account? This revokes this Google access (unless Mail still uses the same account) and deletes its Assistant mail cache, Calendar events and briefings on this Mac. Mail workspace copies and other accounts are kept.", "是否切換主 Google 帳戶？此操作會撤銷目前的 Google 連接（郵件工作區仍使用同一帳戶時除外），並刪除此 Mac 上該帳戶的助手郵件快取、日曆活動及簡報；郵件工作區副本及其他帳戶會保留。")
    )) {
      return;
    }
    setConnecting(true);
    googleCancelled.current = false;
    setGoogleError(null);
    setGoogleNotice(null);
    try {
      const result = await googleConnect(force);
      setGoogleNotice(
        result === GOOGLE_ALREADY_CONNECTED
          ? text("That account is already linked. Use Switch account to change it.", "此帳戶已連接。請使用「切換帳戶」更換。")
          : result
      );
    } catch (e) {
      // Cancelling a sign-in the user abandoned is not a failure.
      if (googleCancelled.current) setGoogleNotice(text("Google sign-in cancelled.", "已取消 Google 登入。"));
      else setGoogleError(String(e));
    } finally {
      await refreshGoogle();
      setConnecting(false);
    }
  }

  // Closing the browser tab tells JARVIS nothing; this stops the wait.
  function cancelGoogleConnect() {
    googleCancelled.current = true;
    void googleCancelConnect().catch(() => {});
  }

  async function handleGoogleDisconnect() {
    if (!window.confirm(
      text("Disconnect the main Google account? This revokes this Google access (unless Mail still uses the same account) and deletes its Assistant mail cache, Calendar events and briefings on this Mac. Mail workspace copies and other accounts are kept. You can reconnect later.", "是否中斷主 Google 帳戶連接？此操作會撤銷 Google 連接（郵件工作區仍使用同一帳戶時除外），並刪除此 Mac 上該帳戶的助手郵件快取、日曆活動及簡報；郵件工作區副本及其他帳戶會保留。之後可以重新連接。")
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
      connectionsChanged();
    } catch (e) { setIntegrationError(String(e)); }
  }

  async function handleSaveGitHub() {
    if (!githubToken.trim()) return;
    setIntegrationError(null);
    try {
      await saveGitHubToken(githubToken.trim());
      setGithubToken("");
      setGithubSaved(true);
      connectionsChanged();
    } catch (e) { setIntegrationError(String(e)); }
  }

  async function handleSaveObsidian() {
    if (!obsidianKey.trim()) return;
    setIntegrationError(null);
    try {
      await saveObsidianKey(obsidianKey.trim());
      setObsidianKey("");
      setObsidianSaved(true);
      connectionsChanged();
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
    const cloud = geminiSpeech;
    const local = Boolean(wakeStatus?.model_downloaded);
    if (cloud && local) return text("Speech-to-text ready -- cloud Gemini, with the local Whisper model as fallback.", "語音辨識可用：雲端 Gemini，並以本機 Whisper 作後備。");
    if (cloud) return text("Speech-to-text ready -- cloud Gemini.", "語音辨識可用：雲端 Gemini。");
    if (local) return text("Speech-to-text ready -- local Whisper model.", "語音辨識可用：本機 Whisper 模型。");
    return chosenAi && chosenAi.provider !== "gemini"
      ? text(`Voice input uses a speech model on this Mac, so your voice does not go to ${chosenAi.label}. Download it once (about 148 MB).`, `語音輸入使用此 Mac 上的語音模型，聲音不會傳送至 ${chosenAi.label}。請先下載一次（約 148 MB）。`)
      : text("Voice input needs a Gemini key (Settings → Your AI) or the speech model on this Mac (about 148 MB).", "語音輸入需要 Gemini 密鑰（設定 → 你的 AI），或此 Mac 上的語音模型（約 148 MB）。");
  }

  function getWakeStatusLine(state: VoiceState): string {
    if (typeof state === "object" && "ModelDownloading" in state) {
      return `${text("Downloading local model", "正在下載本機模型")}：${state.ModelDownloading}%`;
    }
    if (typeof state === "object" && "Error" in state) {
      return `${text("Error", "錯誤")}：${state.Error}`;
    }
    if (state === "WakeWordListening") return text("Wake word listening is active", "正在聆聽喚醒詞");
    if (state === "WakeWordDetected") return text("Wake word detected", "偵測到喚醒詞");
    if (state === "WakeWordProcessing") return text("Processing wake-word request", "正在處理喚醒詞指示");
    if (state === "WakeWordSpeaking") return text("Speaking wake-word response", "正在朗讀回覆");
    if (wakeStatus?.model_downloaded) return text("Local speech model ready", "本機語音模型已準備好");
    return text("Model required before wake-word listening can start", "啟動喚醒詞聆聽前需要先下載模型");
  }

  if (!loaded) {
    return (
      <div style={{ padding: 24 }}>
        {loadError ? <>
          <p role="alert" className="settings-help">{text('Could not load settings. Open the desktop app and try again.', '未能載入設定。請開啟桌面 App 再試。')}</p>
          <button style={styles.actionBtn} onClick={() => { void refreshSettings(); }}>{text('Retry', '重試')}</button>
        </> : <p role="status" className="settings-help">{text('Loading settings…', '正在載入設定…')}</p>}
      </div>
    );
  }

  const options = [
    { value: "gemini_primary", label: text("Gemini Flash (routine) + Gemini Pro (fallback)", "Gemini Flash（日常）＋Gemini Pro（後備）") },
    { value: "gemini_flash", label: text("Gemini Flash (primary) + Pro (fallback)", "Gemini Flash（主要）＋Pro（後備）") },
    { value: "gemini_pro_only", label: text("Gemini Pro only", "只用 Gemini Pro") },
    { value: "gemini_flash_only", label: text("Gemini Flash only", "只用 Gemini Flash") },
  ];

  const modelDownloading =
    typeof voiceState === "object" && "ModelDownloading" in voiceState
      ? voiceState.ModelDownloading
      : null;
  const wakeCanEnable = Boolean(wakeStatus?.model_downloaded || wakeStatus?.enabled);
  const geminiSpeech = Boolean(apiKeyStatus["gemini"]) && (!chosenAi || chosenAi.provider === "gemini");
  const voiceInputReady = geminiSpeech || Boolean(wakeStatus?.model_downloaded);

  const SECTIONS = [
    { id: "account", label: t("accountSection"), Icon: () => <UserRound size={17} /> },
    { id: "ai", label: text("Models", "模型"), Icon: () => <Cpu size={17} /> },
    { id: "keys", label: t("apiKeys"), Icon: () => <Key size={17} /> },
    { id: "google", label: text("Assistant Google", "助手 Google"), Icon: () => <Globe size={17} /> },
    { id: "notion", label: "Notion", Icon: () => <NotionMark size={17} /> },
    { id: "github", label: "GitHub", Icon: () => <GithubMark size={17} /> },
    { id: "obsidian", label: "Obsidian", Icon: () => <Gem size={17} /> },
    { id: "voice", label: text("Voice", "語音"), Icon: () => <Mic size={17} /> },
    { id: "assistant", label: t("navAssistant"), Icon: () => <Bot size={17} /> },
    { id: "appearance", label: text("Appearance", "外觀"), Icon: () => <Palette size={17} /> },
    { id: "storage", label: text("Storage", "儲存空間"), Icon: () => <HardDrive size={17} /> },
  ];

  // Mirrors the sidebar: the Account section only appears once accounts are
  // known to be available in this build (see accountSectionVisible).
  const visibleSections = SECTIONS.filter((s) => s.id !== "account" || accountSectionVisible(accountInfo));

  return (
    <div className="settings-workspace">
      <nav className="settings-nav" aria-label={text('Settings categories', '設定分類')}>
        <div className="settings-nav-title">{t("navSettings")}</div>
        {visibleSections.map((s) => {
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
          <h1>{visibleSections.find((item) => item.id === section)?.label ?? t("navSettings")}</h1>
          <p>{text("Manage how JARVIS works on this Mac.", "管理 JARVIS 在此 Mac 上的運作方式。")}</p>
        </header>
        {integrationError && ["notion", "github", "obsidian"].includes(section) && <p role="alert" style={styles.errorText}>{integrationError}</p>}
        {section === "account" && <AccountPanel />}
        {section === "ai" && (<>
          <div className="settings-info-row">
            <p>{text('The AI you choose here answers the assistant and analyses mail. Mail analysis can use a different AI in its own settings.', '你在此選擇的 AI 會處理助手對話及電郵分析。電郵分析亦可在其設定中另選 AI。')}</p>
            <button style={styles.saveBtn} onClick={() => open("email")}>{text('Mail analysis settings', '郵件分析設定')}</button>
          </div>
          {llmError && <p role="alert" style={styles.errorText}>{llmError}</p>}
          <div className="settings-panel">
            <h2 className="settings-heading">{text('Your AI', '你的 AI')}</h2>
            <ChooseAi onChosen={() => { getProviderChain().then(setChain).catch(() => {}); }} />
          </div>
          <div className="settings-panel">
            <h2 className="settings-heading">{text('Provider priority', '供應商優先次序')}</h2>
            <div style={styles.hint}>{text('JARVIS tries enabled providers in this order. Use the arrows to change priority.', 'JARVIS 會依此次序嘗試已啟用的供應商。可使用箭頭調整優先次序。')}</div>

            {chain.map((entry, i) => {
              const label = entry.provider_type === "local"
                ? `${entry.model_id || text("unknown", "未知")} (${text("local", "本機")})`
                : entry.provider_type === "gemini"
                ? `Gemini (${entry.model_id || text("default", "預設")})`
                : entry.provider_type === "claude"
                ? `Claude (${entry.model_id || text("default", "預設")})`
                : entry.provider_type === "openai" && entry.base_url
                ? `${aiServiceLabel(entry.base_url)} (${entry.model_id || text("default", "預設")})`
                : entry.provider_type === "openai"
                ? `OpenAI (${entry.model_id || text("default", "預設")})`
                : entry.provider_type === "bedrock"
                ? `Bedrock (${entry.model_id || text("default", "預設")})`
                : entry.provider_type;
              return (
                <div key={i} className="settings-chain-row">
                  <span style={{ color: "var(--text-muted)", fontSize: 12, fontFamily: "var(--font-mono)", width: 16 }}>{i + 1}.</span>
                  <span style={{ color: entry.enabled ? "var(--text-primary)" : "var(--text-secondary)", fontSize: 13, minWidth: 0, overflowWrap: "anywhere" }}>{label}</span>
                  <button aria-label={`${text("Move", "上移")} ${label}`} onClick={() => handleMoveChainEntry(i, "up")} disabled={i === 0} style={{ ...styles.saveBtn, fontSize: 12, padding: "1px 5px" }}>↑</button>
                  <button aria-label={`${text("Move down", "下移")} ${label}`} onClick={() => handleMoveChainEntry(i, "down")} disabled={i === chain.length - 1} style={{ ...styles.saveBtn, fontSize: 12, padding: "1px 5px" }}>↓</button>
                  <label style={{ cursor: "pointer" }}>
                    <input type="checkbox" aria-label={`${text("Enable", "啟用")} ${label}`} checked={entry.enabled} onChange={() => handleToggleChainEntry(i)} style={styles.radio} />
                  </label>
                  <button aria-label={`${text("Remove from priority", "從優先次序移除")} ${label}`} onClick={() => handleRemoveFromChain(i)} style={{ ...styles.saveBtn, fontSize: 12, padding: "1px 5px", borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}>{text('Remove', '移除')}</button>
                </div>
              );
            })}

            {chain.length === 0 && (
              <div style={{ color: "var(--text-muted)", fontSize: 13, marginBottom: 8 }}>{text('No providers configured. Add cloud or local providers below.', '尚未設定供應商。請在下方加入雲端或本機供應商。')}</div>
            )}

            <div style={{ display: "flex", gap: 6, marginTop: 8, flexWrap: "wrap" }}>
              <button disabled={!models} onClick={() => models && handleAddToChain("gemini", undefined, models.defaults.gemini_pro)} style={styles.saveBtn}>+ Gemini Pro</button>
              <button disabled={!models} onClick={() => models && handleAddToChain("gemini", undefined, models.defaults.gemini_flash)} style={styles.saveBtn}>+ Gemini Flash</button>
              <button onClick={() => handleAddToChain("claude")} style={styles.saveBtn}>+ Claude</button>
              <button onClick={() => handleAddToChain("openai")} style={styles.saveBtn}>+ OpenAI</button>
              <button disabled={!models} onClick={() => models && handleAddToChain("bedrock", undefined, models.defaults.bedrock)} style={styles.saveBtn}>+ Bedrock Claude</button>
            </div>
          </div>

          <div className="settings-panel">
            <h2 className="settings-heading">{text('Default provider', '預設供應商')}</h2>
            <div style={styles.hint}>{text('Used when no provider chain is configured', '未設定供應商次序時使用')}</div>
            {options.map((opt) => (
              <label key={opt.value} style={styles.option}>
                <input type="radio" name="ai_provider" value={opt.value} checked={aiProvider === opt.value} onChange={() => handleProviderChange(opt.value)} style={styles.radio} />
                <span style={styles.optionLabel}>{opt.label}</span>
              </label>
            ))}
          </div>
          <details className="settings-local-details">
            <summary>{text('Local AI setup and endpoints', '本機 AI 設定及端點')}</summary>
            <p className="settings-help">{text('Optional. Connect a local server or download a model only when you want to use local AI.', '此為選用設定。如需使用本機 AI，請連接伺服器或下載模型。')}</p>
            <div className="settings-local-setup">
              <LocalAiSetup
                endpoints={endpoints}
                onEndpointAdded={(ep) => setEndpoints((prev) => [...prev, ep])}
                onModelsLoaded={(eid, models) => setEndpointModels((prev) => ({ ...prev, [eid]: models }))}
                onAddToChain={handleAddToChain}
              />
            </div>
          <div className="settings-panel">
            <h2 className="settings-heading">{text('Local model endpoints', '本機模型端點')}</h2>
            <div style={styles.hint}>{text('Connect to Ollama, vLLM, or any compatible server', '連接 Ollama、vLLM 或者相容伺服器')}</div>

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
                    {endpointHealth[ep.id].reachable ? `${endpointHealth[ep.id].model_count} ${text("models", "個模型")}，${endpointHealth[ep.id].latency_ms}ms` : text("Unreachable", "無法連接")}
                  </div>
                )}
                {endpointModels[ep.id] && endpointModels[ep.id].length > 0 && (
                  <div style={{ marginTop: 6 }}>
                    <div style={{ color: "var(--text-muted)", fontSize: 12 }}>{text('Available models', '可用模型')}</div>
                    {endpointModels[ep.id].map((m) => (
                      <div key={m.id} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginTop: 2 }}>
                        <span style={{ color: "var(--text-secondary)", fontSize: 12, fontFamily: "var(--font-mono)" }}>{m.id}</span>
                        <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
                          <span style={{ color: "var(--text-muted)", fontSize: 12 }}>{m.supports_tools === "native" ? text("tools", "工具") : m.supports_tools === "prompt_injected" ? text("prompt-tools", "提示工具") : text("chat", "對話")}</span>
                          <button onClick={() => handleAddToChain("local", ep.id, m.id)} style={{ ...styles.saveBtn, fontSize: 12, padding: "2px 6px" }}>{text('Add to priority', '加入優先次序')}</button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
                <div style={{ display: "flex", gap: 6, marginTop: 8 }}>
                  <button onClick={() => handleTestEndpoint(ep.id)} style={{ ...styles.saveBtn, fontSize: 12 }}>{text('Test connection', '測試連接')}</button>
                  <button onClick={() => handleRemoveEndpoint(ep.id)} style={{ ...styles.saveBtn, fontSize: 12, borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}>{text('Remove', '移除')}</button>
                </div>
              </div>
            ))}

            <div style={{ display: "flex", gap: 6, marginTop: 8, flexDirection: "column" as const }}>
              <input aria-label={text('Endpoint name', '端點名稱')} value={newEndpointName} onChange={(e) => setNewEndpointName(e.target.value)} placeholder={text('Name (optional)', '名稱（可選）')} style={styles.tokenInput} />
              <div style={{ display: "flex", gap: 6 }}>
                <input aria-label={text('Endpoint URL', '端點網址')} value={newEndpointUrl} onChange={(e) => setNewEndpointUrl(e.target.value)} placeholder="http://localhost:11434" style={{ ...styles.tokenInput, flex: 1 }} />
                <button onClick={handleAddEndpoint} disabled={endpointBusy} style={styles.saveBtn}>{endpointBusy ? "..." : text('Add endpoint', '加入端點')}</button>
              </div>
            </div>
          </div>
          </details>
        </>)}

        {section === "keys" && (
          <div className="settings-panel">
            <h2 className="settings-heading">{text("Provider API keys", "供應商 API 密鑰")}</h2>
            <div style={styles.hint}>{t("keyHelp")}</div>
            {API_KEY_PROVIDERS.map(({ id, label, help }) => (
              <div key={id} style={{ marginBottom: 12 }}>
                <div style={styles.hint}>
                  {label}
                  {apiKeyStatus[id] ? <span style={{ color: "var(--text-secondary)" }}> · {t("keyAvailable")}</span> : null}
                </div>
                <div style={{ display: "flex", gap: 8 }}>
                  <input
                    type="password"
                    aria-label={`${label} ${text("API key", "API 密鑰")}`}
                    autoComplete="off"
                    style={styles.tokenInput}
                    placeholder={apiKeyStatus[id] ? text("Replace saved key", "取代已儲存密鑰") : text(help, id === "gemini" ? "Gemini 對話及語音辨識" : id === "claude" ? "Claude 對話及後備供應商" : "OpenAI 對話及郵件分析")}
                    value={apiKeyDrafts[id] ?? ""}
                    onChange={(e) => setApiKeyDrafts((d) => ({ ...d, [id]: e.target.value }))}
                  />
                  <button
                    onClick={() => { void handleSaveApiKey(id); }}
                    disabled={!(apiKeyDrafts[id] ?? "").trim()}
                    style={styles.saveBtn}
                  >{t("save")}</button>
                  {apiKeyStatus[id] && (
                    <button
                      onClick={() => { void handleClearApiKey(id, label); }}
                      style={{ ...styles.saveBtn, borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}
                    >{text("Remove key", "移除密鑰")}</button>
                  )}
                </div>
              </div>
            ))}
            <div style={{ ...styles.hint, marginTop: 4, marginBottom: 0 }}>
              {text("Saved keys are never displayed. Leave a field blank to keep the current key, or enter a new one to replace it. Removing deletes the saved key.", "已儲存的密鑰不會顯示。留空可保留現有密鑰；輸入新密鑰則會取代現有密鑰。移除操作會刪除已儲存的密鑰。")}
            </div>
            {apiKeyError && <div style={styles.errorText}>{apiKeyError}</div>}
          </div>
        )}

        {section === "google" && (
          <div className="settings-panel">
            <h2 className="settings-heading">{text('Google for Assistant and Calendar', '助手及日曆的 Google 連接')}</h2>
            <div style={styles.hint}>{text('The main account powers Assistant mail tools, Calendar and briefings, and is the only one JARVIS writes to (drafts and new events). Other accounts add their mail and calendar, read-only.', '主帳戶供助手郵件工具、日曆及簡報使用，亦是 JARVIS 唯一會寫入的帳戶（草稿及新活動）。其他帳戶會加入其郵件及日曆，權限為唯讀。')}</div>
            <h3 style={{ margin: "18px 0 8px", fontSize: 15 }}>{text('Main account', '主帳戶')}</h3>
            {google === null ? (
              <div style={styles.hint}>
                {googleError ? text('Google connection status is unavailable.', '未能取得 Google 連接狀態。') : text('Checking Google connection…', '正在檢查 Google 連接…')}
                {googleError && <button style={{ ...styles.saveBtn, marginLeft: 10 }} onClick={() => { setGoogleError(null); void refreshGoogle(); }}>{text('Retry', '重試')}</button>}
              </div>
            ) : google.main.state !== "not_connected" ? (
              <>
                <div style={{ color: google.main.state === "reconnect" ? "var(--accent-urgent)" : "var(--text-secondary)", fontSize: 12, marginBottom: 10 }}>
                  {google.main.state === "reconnect"
                    ? text("Google no longer accepts this connection. Reconnect to keep using mail tools, Calendar and briefings.", "Google 已不接受此連接。請重新連接，以繼續使用郵件工具、日曆及簡報。")
                    : mainLabel ? `${text("Connected as", "已連接帳戶：")} ${mainLabel}` : text('Connected', '已連接')}
                </div>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  {google.main.state === "reconnect" && <button
                    onClick={() => { void handleGoogleConnect(false); }}
                    disabled={connecting || disconnecting}
                    style={{ ...styles.actionBtn, borderColor: "var(--cyan)" }}
                  >
                    {connecting ? text('Connecting…', '正在連接…') : text('Reconnect', '重新連接')}
                  </button>}
                  <button
                    onClick={() => { void handleGoogleConnect(true); }}
                    disabled={connecting || disconnecting}
                    style={styles.actionBtn}
                  >
                    {connecting && google.main.state !== "reconnect" ? text('Opening…', '正在開啟…') : text('Switch account', '切換帳戶')}
                  </button>
                  <button
                    onClick={() => { void handleGoogleDisconnect(); }}
                    disabled={connecting || disconnecting}
                    style={{ ...styles.actionBtn, borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}
                  >
                    {disconnecting ? text('Disconnecting…', '正在中斷連接…') : text('Disconnect', '斷開')}
                  </button>
                  {connecting && <button onClick={cancelGoogleConnect} style={styles.actionBtn}>{text('Cancel', '取消')}</button>}
                </div>
                <div style={{ ...styles.hint, marginTop: 8, marginBottom: 0 }}>
                  {text("Disconnecting removes this account's Assistant mail cache, Calendar events and briefings. Local copies in the Mail workspace and other accounts are kept.", "中斷連接會刪除此帳戶的助手郵件快取、日曆活動及簡報；郵件工作區的本機副本及其他帳戶會保留。")}
                </div>
              </>
            ) : (
              <div style={{ display: "flex", gap: 8 }}>
                <button onClick={() => { void handleGoogleConnect(false); }} disabled={connecting} style={styles.actionBtn}>
                  {connecting ? text('Connecting…', '正在連接…') : text('Connect Google', '連接 Google')}
                </button>
                {connecting && <button onClick={cancelGoogleConnect} style={styles.actionBtn}>{text('Cancel', '取消')}</button>}
              </div>
            )}
            {connecting && <div style={{ ...styles.hint, marginTop: 8, marginBottom: 0 }}>{text('Finish signing in in your browser. If you closed the Google tab, choose Cancel.', '請在瀏覽器完成登入。如已關閉 Google 登入分頁，請按「取消」。')}</div>}
            {googleNotice && <div style={{ ...styles.hint, marginTop: 10, marginBottom: 0 }}>{googleNotice}</div>}
            {googleError && <div style={styles.errorText}>{googleError}</div>}
            {google && <OtherGoogleAccounts accounts={google.others} canAdd={google.main.state === "connected"} onChanged={refreshGoogle} />}
          </div>
        )}

        {section === "notion" && (
          <div className="settings-panel">
            <IntegrationConnection service="notion" />
            <details className="integration-advanced"><summary>{text("Advanced: use an API token", "進階：使用 Notion 整合權杖")}</summary>
            <div style={styles.hint}>{text('Enter your Notion integration token', '輸入 Notion integration token')}</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" aria-label={text('Notion integration token', 'Notion integration token')} autoComplete="off" value={notionToken} onChange={(e) => { setNotionToken(e.target.value); setNotionSaved(false); }} placeholder="ntn_..." style={styles.tokenInput} />
              <button disabled={!notionToken.trim()} onClick={handleSaveNotion} style={styles.saveBtn}>{notionSaved ? t("saved") : t("save")}</button>
            </div>
            </details>
          </div>
        )}

        {section === "github" && (
          <div className="settings-panel">
            <IntegrationConnection service="github" />
            <details className="integration-advanced"><summary>{text("Advanced: use an API token", "進階：使用個人存取權杖")}</summary>
            <div style={styles.hint}>{text('Enter your GitHub personal access token', '輸入 GitHub personal access token')}</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" aria-label={text('GitHub personal access token', 'GitHub personal access token')} autoComplete="off" value={githubToken} onChange={(e) => { setGithubToken(e.target.value); setGithubSaved(false); }} placeholder="ghp_..." style={styles.tokenInput} />
              <button disabled={!githubToken.trim()} onClick={handleSaveGitHub} style={styles.saveBtn}>{githubSaved ? t("saved") : t("save")}</button>
            </div>
            </details>
          </div>
        )}

        {section === "obsidian" && (
          <div className="settings-panel">
            <IntegrationConnection service="obsidian" />
            <details className="integration-advanced"><summary>{text("Advanced: use an API token", "進階：使用 Local REST API 外掛")}</summary>
            <div style={styles.hint}>{text('Connect to Obsidian vault via Local REST API plugin', '透過 Local REST API 外掛連接 Obsidian 資料庫')}</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" aria-label={text('Obsidian API key', 'Obsidian API 密鑰')} autoComplete="off" value={obsidianKey} onChange={(e) => { setObsidianKey(e.target.value); setObsidianSaved(false); }} placeholder="API key from Obsidian REST API plugin" style={styles.tokenInput} />
              <button disabled={!obsidianKey.trim()} onClick={handleSaveObsidian} style={styles.saveBtn}>{obsidianSaved ? t("saved") : t("save")}</button>
            </div>
            <div style={{ color: "var(--text-muted)", fontSize: 12, marginTop: 6 }}>
              {text('Install and enable the "Local REST API" plugin in Obsidian, then copy its API key.', '請在 Obsidian 安裝並啟用「Local REST API」外掛，再複製其 API key。')}
            </div>
            <div style={{ marginTop: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text('Endpoint', '端點')}</div>
              <input
                aria-label={text('Obsidian endpoint URL', 'Obsidian 端點網址')}
                value={obsidianBaseUrl}
                onChange={(e) => setObsidianBaseUrl(e.target.value)}
                onBlur={() => { void setPreference("obsidian_base_url", obsidianBaseUrl.trim()).then(connectionsChanged).catch(e => setIntegrationError(String(e))); }}
                placeholder="https://127.0.0.1:27124"
                style={styles.tokenInput}
              />
              <div style={{ color: "var(--text-muted)", fontSize: 12, marginTop: 6 }}>
                {text("Leave blank for the plugin default (HTTPS on port 27124). Change this only if you configured a different local endpoint.", "留空會使用外掛預設的 HTTPS 27124 連接埠。僅在已設定其他本機端點時更改。")}
              </div>
            </div>
            </details>
          </div>
        )}

        {section === "ai" && (
          <div className="settings-panel">
            <h2 className="settings-heading">AWS Bedrock</h2>
            <div style={styles.hint}>
              {text("Use a model and region available to your AWS account. Mail analysis has its own model setting. Saving credentials does not test access.", "請使用 AWS 帳戶可用的模型及地區。電郵分析設有獨立模型設定；儲存憑證不代表已驗證連接。")}
              {awsStatus.configured && (
                <> {text("Saved region", "已儲存地區")} <code style={{ fontFamily: "var(--font-mono)", color: "var(--text-primary)" }}>{awsStatus.region}</code>.</>
              )}
            </div>
            <div style={{ marginBottom: 8 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text('Access Key ID', '存取密鑰 ID')}</div>
              <input aria-label={text('AWS access key ID', 'AWS 存取密鑰 ID')} value={awsAccessKey} onChange={(e) => setAwsAccessKey(e.target.value)} placeholder="AKIA..." style={styles.tokenInput} />
            </div>
            <div style={{ marginBottom: 8 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text('Secret Access Key', '秘密存取密鑰')}</div>
              <input type="password" aria-label={text('AWS secret access key', 'AWS 秘密存取密鑰')} autoComplete="off" value={awsSecret} onChange={(e) => setAwsSecret(e.target.value)} placeholder="Secret key" style={styles.tokenInput} />
            </div>
            <div style={{ marginBottom: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text('Region', '地區')}</div>
              <select aria-label={text("AWS region", "AWS 地區")} value={awsRegion} onChange={(e) => setAwsRegion(e.target.value)} style={styles.select}>
                <option value="ap-southeast-1">{text("Singapore (ap-southeast-1)", "新加坡（ap-southeast-1）")}</option>
                <option value="ap-northeast-1">{text("Tokyo (ap-northeast-1)", "東京（ap-northeast-1）")}</option>
                <option value="us-east-1">{text("US East (us-east-1)", "美國東部（us-east-1）")}</option>
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
              }} style={styles.saveBtn}>{t("save")}</button>
              {awsStatus.configured && (
                <button onClick={async () => {
                  await clearAwsCredentials();
                  setAwsStatus({ configured: false, region: null });
                }} style={{ ...styles.saveBtn, borderColor: "var(--accent-urgent)", color: "var(--accent-urgent)" }}>{text("Remove key", "移除密鑰")}</button>
              )}
            </div>
            {awsError && <div style={styles.errorText}>{awsError}</div>}
          </div>
        )}

        {section === "voice" && (<>
          <div className="settings-panel">
            <h2 className="settings-heading">{text("Assistant language", "助手語言")}</h2>
            <div style={styles.hint}>
              {text("Controls the assistant's response language and speech-to-text language.", "以下分別設定助手回覆及語音辨識的語言。")}
            </div>

            <div style={{ marginTop: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text("Response language", "回覆語言")}</div>
              <select aria-label={text("Response language", "回覆語言")} value={language} style={styles.select} onChange={async (e) => {
                const v = e.target.value;
                setLanguage(v);
                await setPreference("language", v);
              }}>
                <option value="en">English</option>
                <option value="yue">{text("Traditional Chinese (written)", "繁體書面語")}</option>
                <option value="bilingual">{text("Bilingual (code-mix)", "雙語混合")}</option>
              </select>
            </div>

            <div style={{ marginTop: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text("Speech-to-text language", "語音辨識語言")}</div>
              <select aria-label={text("Speech-to-text language", "語音辨識語言")} value={sttLanguage} style={styles.select} onChange={async (e) => {
                const v = e.target.value;
                setSttLanguage(v);
                await setPreference("stt_language", v);
              }}>
                <option value="auto">{text("Auto-detect", "自動偵測")}</option>
                <option value="en">English</option>
                <option value="yue">廣東話</option>
              </select>
            </div>
          </div>

          <div className="settings-panel">
            <h2 className="settings-heading">{text('Speech', '語音')}</h2>
            <div style={styles.hint}>{text('Cmd+Shift+J to start/stop voice input', '按 Cmd+Shift+J 開始或結束語音輸入')}</div>
            <div style={voiceInputReady ? styles.hint : styles.statusText}>{getVoiceAvailabilityLine()}</div>
            {!voiceInputReady && <button onClick={() => { void handleDownloadModel(false); }} disabled={wakeBusy || modelDownloading !== null} style={{ ...styles.actionBtn, margin: "8px 0 12px" }}>{modelDownloading !== null ? `${text("Downloading", "正在下載")} ${modelDownloading}%` : text("Download the speech model (about 148 MB)", "下載語音模型（約 148 MB）")}</button>}
            <label style={styles.option}>
              <input type="checkbox" checked={voiceSettings?.tts_enabled ?? true} onChange={(e) => { void setVoiceSetting("tts_enabled", String(e.target.checked)); setVoiceSettingsState((prev) => prev ? { ...prev, tts_enabled: e.target.checked } : prev); }} style={styles.radio} />
              <span style={styles.optionLabel}>{text('Enable text-to-speech', '啟用文字朗讀')}</span>
            </label>
            {ttsVoices.length > 0 && (
              <div style={{ marginTop: 8 }}>
                <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text('TTS Voice', '朗讀聲線')}</div>
                <select aria-label={text('Speech voice', '朗讀聲線')} value={voiceSettings?.tts_voice ?? "Samantha"} onChange={(e) => { void setVoiceSetting("tts_voice", e.target.value); setVoiceSettingsState((prev) => prev ? { ...prev, tts_voice: e.target.value } : prev); }} style={styles.select}>
                  {ttsVoices.map((voice) => <option key={voice} value={voice}>{voice}</option>)}
                </select>
              </div>
            )}
            {ttsVoices.length > 0 && (
              <div style={{ marginTop: 8 }}>
                <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text('Cantonese voice', '廣東話聲線')}</div>
                <select aria-label={text('Cantonese voice', '廣東話聲線')} value={ttsVoiceYue} onChange={(e) => { void setVoiceSetting("tts_voice_yue", e.target.value); setTtsVoiceYue(e.target.value); }} style={styles.select}>
                  {ttsVoices.map((voice) => <option key={voice} value={voice}>{voice}</option>)}
                </select>
              </div>
            )}
          </div>

          <div className="settings-panel">
            <h2 className="settings-heading">{text('Wake word', '喚醒詞')}</h2>
            <div style={styles.hint}>{text('Experimental. Listen locally for "Hey Jarvis" to start voice input.', '實驗功能：在本機偵測「Hey Jarvis」以啟動語音輸入。')}</div>
            <label style={styles.option}>
              <input type="checkbox" checked={wakeStatus?.running ?? false} disabled={!wakeCanEnable || !canSwitchListening(wakeStatus, wakeBusy, voiceState)} onChange={(e) => { void handleWakeToggle(e.target.checked); }} style={styles.radio} />
              <span style={styles.optionLabel}>{text('Enable wake word', '啟用喚醒詞')}</span>
            </label>
            <div style={styles.statusText}>{getWakeStatusLine(voiceState)}</div>
            <div style={{ display: "flex", gap: 8 }}>
              <button onClick={() => { void handleDownloadModel(false); }} disabled={wakeBusy || modelDownloading !== null || wakeStatus?.model_downloaded} style={styles.actionBtn}>
                {modelDownloading !== null ? text('Downloading…', '正在下載…') : wakeStatus?.model_downloaded ? text('Model ready', '模型已準備好') : text('Download model', '下載模型')}
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
                {text("Model file: ~/Library/Application Support/jarvis/models/ggml-base.bin. If transcription keeps failing to load the model, that file is damaged: Download again replaces it in place.", "模型檔案：~/Library/Application Support/jarvis/models/ggml-base.bin。如果語音辨識持續無法載入模型，檔案可能已損壞；「重新下載」會取代原檔。")}
              </div>
            )}
            <div style={styles.privacyText}>
              {text("Wake-word detection runs on this Mac. After it wakes, the full request uses cloud speech-to-text only when Gemini is the AI you chose; otherwise it stays on this Mac.", "喚醒詞偵測會在本機處理音訊。喚醒後，只有當你選擇的 AI 是 Gemini 時，完整指示才會使用雲端語音辨識；否則只在此 Mac 處理。")}
            </div>
            {wakeError && <div style={styles.errorText}>{wakeError}</div>}
          </div>
        </>)}

        {section === "assistant" && (<>
          <div className="settings-panel">
            <h2 className="settings-heading">{text('Personalisation', '個人化')}</h2>
            <div style={styles.hint}>{text('How JARVIS talks to you, in typed and spoken replies.', '設定 JARVIS 在文字及語音回覆中與你對話的方式。')}</div>
            <div style={{ marginTop: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text('What JARVIS calls you', 'JARVIS 對你的稱呼')}</div>
              <input aria-label={text('What JARVIS calls you', 'JARVIS 對你的稱呼')} value={personaText.assistant_address} maxLength={MAX_ADDRESS_CHARS} placeholder={displayName || text('Your name', '你的名稱')} style={styles.tokenInput} onChange={(e) => { setPersonaText(p => ({ ...p, assistant_address: e.target.value })); setPersonaSaved(false); }} />
              <div style={{ ...styles.hint, marginTop: 6 }}>{text('Leave blank to use your name. JARVIS uses it now and then, not in every reply.', '留空則使用你的名稱。JARVIS 只會在適當時候稱呼你，不會每句都用。')}</div>
            </div>
            {([["assistant_tone", text('Tone', '語氣')], ["assistant_length", text('Reply length', '回覆長度')], ["assistant_chinese", text('Chinese replies', '中文回覆')], ["assistant_search", text('Search depth', '搜尋深度')]] as [PersonaKey, string][]).map(([key, label]) => (
              <div key={key} style={{ marginTop: 12 }}>
                <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{label}</div>
                <select aria-label={label} value={personaValue(key, persona[key] ?? null)} style={styles.select} onChange={async (e) => {
                  const v = e.target.value;
                  setPersona(p => ({ ...p, [key]: v }));
                  await setPreference(key, v);
                }}>
                  {PERSONA_CHOICES[key].map(value => <option key={value} value={value}>{personaLabel(value, text)}</option>)}
                </select>
                {key === "assistant_chinese" && <div style={{ ...styles.hint, marginTop: 6 }}>{text('Emails, events and notes written for other people always use written Chinese.', '寫給其他人的郵件、活動及筆記一律使用書面語。')}</div>}
                {key === "assistant_search" && <div style={{ ...styles.hint, marginTop: 6 }}>{text('Thorough uses a little more of your AI allowance.', '徹底模式會略為增加 AI 用量。')}</div>}
              </div>
            ))}
            <div style={{ marginTop: 12 }}>
              <div style={{ color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{text('Anything else about how you like replies', '其他回覆偏好')}</div>
              <textarea aria-label={text('Anything else about how you like replies', '其他回覆偏好')} value={personaText.assistant_instructions} maxLength={MAX_INSTRUCTIONS_CHARS} rows={4} placeholder={text('For example: lead with the most urgent item.', '例如：先說最緊急的事項。')} style={{ ...styles.tokenInput, resize: "vertical" }} onChange={(e) => { setPersonaText(p => ({ ...p, assistant_instructions: e.target.value })); setPersonaSaved(false); }} />
              <div style={{ ...styles.hint, marginTop: 6 }}>{`${personaText.assistant_instructions.length}/${MAX_INSTRUCTIONS_CHARS} · `}{text('Tone and format only. It cannot change what JARVIS is allowed to do.', '只影響語氣及格式，不會改變 JARVIS 可以做的事。')}</div>
            </div>
            <button style={styles.saveBtn} onClick={async () => {
              setPersonaError(null);
              try {
                await setPreference("assistant_address", personaText.assistant_address.trim());
                await setPreference("assistant_instructions", personaText.assistant_instructions.trim());
                setPersonaSaved(true);
              } catch (e) { setPersonaError(String(e)); }
            }}>{personaSaved ? t("saved") : t("save")}</button>
            {personaError && <div style={styles.errorText}>{personaError}</div>}
          </div>
          <div className="settings-panel">
            <h2 className="settings-heading">{text('Briefing', '簡報')}</h2>
            <label style={styles.option}>
              <input type="checkbox" checked={autoBriefing} onChange={(e) => { setAutoBriefing(e.target.checked); void updateSetting("auto_briefing", String(e.target.checked)); }} style={styles.radio} />
              <span style={styles.optionLabel}>{text('Speak morning briefing on startup (once per day)', '啟動時朗讀晨早簡報（每日一次）')}</span>
            </label>
          </div>
        </>)}

        {section === "appearance" && (<>
          <div className="settings-panel">
            <h2 className="settings-heading">{t("displayName")}</h2>
            <label style={{ display: "block", marginTop: 12 }}>
              <span style={{ display: "block", color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{t("displayName")}</span>
              <input
                type="text"
                value={displayName}
                placeholder={t("displayNamePlaceholder")}
                aria-label={t("displayName")}
                style={styles.tokenInput}
                onChange={(e) => { setDisplayName(e.target.value); setDisplayNameSaved(false); }}
                onBlur={() => { void saveDisplayName(); }}
              />
            </label>
            <div style={styles.hint}>{t("displayNameHelp")}</div>
            <button style={{ ...styles.saveBtn, marginTop: 4 }} onClick={() => { void saveDisplayName(); }}>{displayNameSaved ? t("saved") : t("save")}</button>
          </div>
          <div className="settings-panel">
            <h2 className="settings-heading">{t("uiLanguage")}</h2>
            <label style={{ display: "block", marginTop: 12 }}>
              <span style={{ display: "block", color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>{t("uiLanguage")}</span>
              <select aria-label={t("uiLanguage")} value={locale} style={styles.select} onChange={(e) => setLocale(e.target.value as "en" | "zh-HK")}>
                <option value="en">English</option><option value="zh-HK">繁體中文（香港）</option>
              </select>
            </label>
            <div style={styles.hint}>{t("uiLanguageHelp")}</div>
            {localeSaveError && <p role="alert" style={styles.errorText}>{text(localeSaveError, "未能儲存介面語言。關閉 JARVIS 前請重試。")} <button onClick={retryLocaleSave}>{t("retry")}</button></p>}
          </div>
          <div className="settings-panel">
            <h2 className="settings-heading">{text('Colour theme', '色彩主題')}</h2>
            <p style={styles.hint}>{text('Choose a light or dark workspace, or follow your Mac.', '選擇淺色、深色，或者跟隨 Mac 外觀。')}</p>
            <div className="settings-appearance-options" role="group" aria-label={text('Colour theme', '色彩主題')}>
              {([
                { value: "light", label: text('Light', '淺色'), help: text('A bright workspace', '淺色工作區') },
                { value: "dark", label: text('Dark', '深色'), help: text('A darker workspace', '深色工作區') },
                { value: "system", label: text('System', '系統'), help: text('Follow macOS appearance', '跟隨 macOS 外觀') },
              ] as const).map((mode) => (
                <button key={mode.value} className={appearance === mode.value ? "is-selected" : ""}
                  aria-pressed={appearance === mode.value} onClick={() => setAppearance(mode.value)}>
                  <strong>{mode.label}</strong><span>{mode.help}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="settings-panel">
            <h2 className="settings-heading">{text('Desktop wallpaper', '桌面背景')}</h2>
            <div style={styles.hint}>
              {text("Turn the full JARVIS app into a live macOS desktop wallpaper. The entire UI renders behind all windows and desktop icons.", "將完整 JARVIS App 顯示為 macOS 動態桌面背景。介面會呈現在其他視窗及桌面圖示後方。")}
            </div>
            <label style={styles.option}>
              <input type="checkbox" checked={wallpaperActive} disabled={wallpaperBusy} onChange={(e) => { void handleWallpaperToggle(e.target.checked); }} style={styles.radio} />
              <span style={styles.optionLabel}>{text('Enable wallpaper mode', '啟用桌面背景模式')}</span>
            </label>
            <div style={styles.statusText}>
              {wallpaperBusy ? text("Switching…", "正在切換…") : wallpaperActive ? text("Wallpaper active — full JARVIS on desktop", "桌面背景已啟用 — 完整 JARVIS 介面") : text("Wallpaper inactive", "桌面背景未啟用")}
            </div>
            {wallpaperError && <p role="alert" style={styles.errorText}>{wallpaperError}</p>}
            <div style={styles.privacyText}>
              {text("When active, JARVIS goes fullscreen behind all windows. After enabling, it stays interactive until you send it to the background. To bring it back later, say Hey Jarvis, click the tray icon, or press Cmd+Shift+W. Closing the window sends it back to the background.", "啟用後，JARVIS 會以全螢幕顯示在其他視窗後方，並保持可互動狀態，直至移至背景。之後可說出「Hey Jarvis」、按選單列圖示，或按 Cmd+Shift+W 重新開啟。關閉視窗會將 JARVIS 移回背景。")}
            </div>
          </div>
        </>)}

        {section === "storage" && (<>
          <div className="settings-panel">
            <h2 className="settings-heading">{text('On this Mac', '本機用量')}</h2>
            <p style={styles.hint}>
              {text(
                "This page shows JARVIS's local copies and files on this Mac. Cloud AI analysis sends the content needed for your request to the selected provider.",
                "此頁顯示 JARVIS 在這部 Mac 儲存的本機副本及檔案。使用雲端 AI 分析時，所需內容會傳送至你選擇的供應商。",
              )}
            </p>
            {!storage && !storageError && <div style={styles.statusText}>{text("Reading…", "正在讀取…")}</div>}
            {storage && (<>
              <div style={styles.storageTotal}>{formatBytes(storage.totalBytes)}</div>
              <p style={styles.hint}>{text(
                "Total for the items listed below. Excludes the App itself and models managed by other apps, such as Ollama.",
                "總計只包括下列項目，不包括 App 本身或由 Ollama 等其他應用程式管理的模型。",
              )}</p>
              <dl style={styles.storageList}>
                {([
                  ["database", text('Database', '資料庫'), storage.databaseBytes + storage.walBytes],
                  ["models", text('Voice and AI models', '語音及 AI 模型'), storage.modelsBytes],
                  ["cache", text('Web cache', '網頁快取'), storage.cacheBytes],
                  ["backups", text('Old backups', '舊備份'), storage.backupsBytes],
                ] as const).map(([key, label, bytes]) => (
                  <div key={key} style={styles.storageRow}>
                    <dt style={styles.storageLabel}>{label}</dt>
                    <dd style={styles.storageValue}>{formatBytes(bytes)}</dd>
                  </div>
                ))}
              </dl>
              <div style={styles.hint}>
                {text(
                  `${storage.mailMessageCount} mail message(s) holding ${formatBytes(storage.mailBodyBytes)} of message text, and ${storage.conversationCount} chat turn(s).`,
                  `已儲存 ${storage.mailMessageCount} 封郵件，內文共 ${formatBytes(storage.mailBodyBytes)}；對話記錄 ${storage.conversationCount} 項。`,
                )}
              </div>
              <div style={styles.privacyText}>{storage.dataDir}</div>
            </>)}
            {storageError && <p role="alert" style={styles.errorText}>{storageError}</p>}
            {storageNotice && <p style={styles.statusText}>{storageNotice}</p>}
          </div>

          <div className="settings-panel">
            <h2 className="settings-heading">{text('Automatic clean-up', '自動清理')}</h2>
            <p style={styles.hint}>
              {text(
                "When enabled, clean-up runs on launch or when you choose Clean up now. Ungrouped mail is aged from its last local save: only its text is removed, while subjects, senders and previews remain. Grouped mail is kept. Old conversations and related cached records are also removed.",
                "啟用後，JARVIS 會在啟動或按「立即清理」時執行清理。未分組郵件以最近儲存日期計算期限，只移除內文，保留主旨、寄件人及預覽；已分組的郵件會保留。超過期限的對話及相關快取記錄亦會移除。",
              )}
            </p>
            <label style={styles.option}>
              <input type="checkbox" style={styles.radio}
                checked={storage?.retentionEnabled ?? true}
                disabled={!storage || !!storageBusy}
                onChange={(e) => handleRetentionChange(storage?.retentionDays ?? 90, e.target.checked)} />
              <span style={styles.optionLabel}>{text('Remove content older than', '自動移除超過此期限的內容')}</span>
            </label>
            <label style={{ display: "block", marginTop: 4 }}>
              <span style={{ display: "block", color: "var(--text-secondary)", fontSize: 12, marginBottom: 4 }}>
                {text('Retention window', '保留期限')}
              </span>
              <select style={styles.select}
                aria-label={text('Retention window', '保留期限')}
                value={String(storage?.retentionDays ?? 90)}
                disabled={!storage || !storage.retentionEnabled || !!storageBusy}
                onChange={(e) => handleRetentionChange(Number(e.target.value), true)}>
                <option value="30">{text('30 days', '30 日')}</option>
                <option value="90">{text('90 days', '90 日')}</option>
                <option value="180">{text('180 days', '180 日')}</option>
                <option value="365">{text('1 year', '1 年')}</option>
              </select>
            </label>
            <div style={{ marginTop: 14 }}>
              <button style={styles.actionBtn} disabled={!storage || !!storageBusy} onClick={handleSweep}>
                {storageBusy || text('Clean up now', '立即清理')}
              </button>
            </div>
          </div>

          <div className="settings-panel">
            <h2 className="settings-heading">{text('Free up space', '釋放空間')}</h2>
            <p style={styles.hint}>
              {text(
                "Choose which local items to remove. Models can be downloaded again and caches rebuilt; deleted backups and chat history cannot be recovered. Removing local mail text does not delete the originals in Gmail.",
                "請選擇要移除的本機項目。模型可重新下載，快取可重建；備份及對話記錄刪除後無法復原。移除本機郵件內文不會刪除 Gmail 的原始郵件。",
              )}
            </p>
            {([
              { scope: "cache", label: text('Clear web cache', '清除網頁快取'),
                help: text('Rebuilt automatically.', '會自動重建。'), bytes: storage?.cacheBytes },
              { scope: "backups", label: text('Delete old backups', '刪除舊備份'),
                help: text('Removes earlier backup copies permanently. Make sure you no longer need them to restore data or an older version.', '永久刪除先前的備份副本。請先確認不再需要用這些備份還原資料或舊版本。'), bytes: storage?.backupsBytes },
              { scope: "models", label: text('Delete downloaded models', '刪除已下載的模型'),
                help: text('Offline speech and voice models. Download them again to use the affected offline voice features.', '離線語音及聲音模型。刪除後，須重新下載才能使用相關的離線語音功能。'), bytes: storage?.modelsBytes },
              { scope: "mailBodies", label: text('Drop stored mail text', '移除已儲存的郵件內文'),
                help: text('Keeps subjects and previews. Grouped mail is kept in full.', '保留主旨及預覽；已分組的郵件會完整保留。'), bytes: storage?.mailBodyBytes },
            ] as const).map((item) => (
              <div key={item.scope} style={styles.storagePurgeRow}>
                <div>
                  <div style={styles.optionLabel}>{item.label}</div>
                  <div style={styles.privacyText}>{item.help}</div>
                </div>
                <button style={styles.saveBtn} disabled={!storage || !!storageBusy || !item.bytes}
                  onClick={() => handlePurge(item.scope as StorageScope, item.label)}>
                  {item.bytes ? formatBytes(item.bytes) : text('Empty', '沒有資料')}
                </button>
              </div>
            ))}
            <div style={styles.storagePurgeRow}>
              <div>
                <div style={styles.optionLabel}>{text('Delete all chat history', '刪除所有對話記錄')}</div>
                <div style={styles.privacyText}>{text('Cannot be undone.', '此操作無法復原。')}</div>
              </div>
              <button style={styles.saveBtn} disabled={!storage || !!storageBusy || !storage?.conversationCount}
                onClick={() => handlePurge("conversations", text('Delete all chat history', '刪除所有對話記錄'))}>
                {storage?.conversationCount
                  ? text(`${storage.conversationCount} turns`, `${storage.conversationCount} 項`)
                  : text('Empty', '沒有資料')}
              </button>
            </div>
          </div>
        </>)}

      </div>
    </div>
  );
}

/// Decimal units, matching what Finder reports, so a user comparing the panel
/// against Get Info sees the same number.
function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log10(bytes) / 3), units.length - 1);
  const value = bytes / 1000 ** index;
  return `${value >= 100 || index === 0 ? Math.round(value) : value.toFixed(1)} ${units[index]}`;
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
  storageTotal: { color: "var(--text-primary)", fontSize: 28, lineHeight: 1.2, marginBottom: 14, fontVariantNumeric: "tabular-nums" },
  storageList: { margin: 0, display: "flex", flexDirection: "column", gap: 8 },
  storageRow: { display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 16 },
  storageLabel: { color: "var(--text-secondary)", fontSize: 13, margin: 0 },
  storageValue: { color: "var(--text-primary)", fontSize: 13, margin: 0, fontVariantNumeric: "tabular-nums" },
  storagePurgeRow: { display: "flex", alignItems: "center", justifyContent: "space-between", gap: 16, paddingTop: 12, marginTop: 12, borderTop: "1px solid var(--border-primary)" },
};
