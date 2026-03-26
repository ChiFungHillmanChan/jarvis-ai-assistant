import { useEffect, useState } from "react";
import {
  getSettings,
  updateSetting,
  googleConnect,
  googleStatus,
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
} from "../lib/commands";
import { listen } from "@tauri-apps/api/event";
import type { VoiceSettings, WakeWordStatus, VoiceState, LocalEndpoint, LocalModel, ProviderChainEntry, EndpointHealth } from "../lib/types";
import { useVoiceState } from "../hooks/useVoiceState";
import LocalAiSetup from "../components/LocalAiSetup";

export default function Settings() {
  const [aiProvider, setAiProvider] = useState("claude_primary");
  const [loaded, setLoaded] = useState(false);
  const [googleConnected, setGoogleConnected] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [googleError, setGoogleError] = useState<string | null>(null);
  const [notionToken, setNotionToken] = useState("");
  const [githubToken, setGithubToken] = useState("");
  const [notionSaved, setNotionSaved] = useState(false);
  const [githubSaved, setGithubSaved] = useState(false);
  const [obsidianKey, setObsidianKey] = useState("");
  const [obsidianSaved, setObsidianSaved] = useState(false);
  const [voiceSettings, setVoiceSettingsState] = useState<VoiceSettings | null>(null);
  const [ttsVoices, setTtsVoices] = useState<string[]>([]);
  const [wakeStatus, setWakeStatus] = useState<WakeWordStatus | null>(null);
  const [wakeBusy, setWakeBusy] = useState(false);
  const [wakeError, setWakeError] = useState<string | null>(null);
  const { state: voiceState } = useVoiceState();
  const [wallpaperActive, setWallpaperActive] = useState(false);
  const [wallpaperBusy, setWallpaperBusy] = useState(false);

  // Local LLM state
  const [endpoints, setEndpoints] = useState<LocalEndpoint[]>([]);
  const [chain, setChain] = useState<ProviderChainEntry[]>([]);
  const [newEndpointUrl, setNewEndpointUrl] = useState("");
  const [newEndpointName, setNewEndpointName] = useState("");
  const [endpointBusy, setEndpointBusy] = useState(false);
  const [endpointModels, setEndpointModels] = useState<Record<string, LocalModel[]>>({});
  const [endpointHealth, setEndpointHealth] = useState<Record<string, EndpointHealth>>({});
  const [llmError, setLlmError] = useState<string | null>(null);

  useEffect(() => {
    getSettings().then((s) => {
      setAiProvider(s.values["ai_provider"] || "claude_primary");
      setLoaded(true);
    });
  }, []);

  useEffect(() => {
    googleStatus().then(setGoogleConnected);
  }, []);

  useEffect(() => {
    getVoiceSettings().then(setVoiceSettingsState);
    listTtsVoices().then(setTtsVoices);
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
      provider_type: providerType as "claude" | "openai" | "local",
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

  async function handleGoogleConnect() {
    setConnecting(true);
    setGoogleError(null);
    try {
      await googleConnect();
      setGoogleConnected(true);
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      setConnecting(false);
    }
  }

  async function handleSaveNotion() {
    if (!notionToken.trim()) return;
    await saveNotionToken(notionToken);
    setNotionSaved(true);
  }

  async function handleSaveGitHub() {
    if (!githubToken.trim()) return;
    await saveGitHubToken(githubToken);
    setGithubSaved(true);
  }

  async function handleSaveObsidian() {
    if (!obsidianKey.trim()) return;
    await saveObsidianKey(obsidianKey);
    setObsidianSaved(true);
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
    try {
      if (enabled) {
        await enableWallpaper();
      } else {
        await disableWallpaper();
      }
      setWallpaperActive(enabled);
      await updateSetting("wallpaper_mode_enabled", String(enabled));
    } catch (e) {
      console.error("Wallpaper toggle failed:", e);
    } finally {
      setWallpaperBusy(false);
    }
  }

  async function handleDownloadModel() {
    setWakeError(null);
    setWakeBusy(true);
    try {
      await downloadModel();
      await refreshWakeStatus();
    } catch (e) {
      setWakeError(String(e));
    } finally {
      setWakeBusy(false);
    }
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
    if (wakeStatus?.model_downloaded) return "Local model ready";
    return "Model required before wake-word listening can start";
  }

  if (!loaded) {
    return (
      <div style={{ padding: 24 }}>
        <div className="system-text animate-glow">LOADING SETTINGS...</div>
      </div>
    );
  }

  const options = [
    { value: "claude_primary", label: "Claude (primary) + OpenAI (fallback)" },
    { value: "openai_primary", label: "OpenAI (primary) + Claude (fallback)" },
    { value: "claude_only", label: "Claude only" },
    { value: "openai_only", label: "OpenAI only" },
  ];

  const modelDownloading =
    typeof voiceState === "object" && "ModelDownloading" in voiceState
      ? voiceState.ModelDownloading
      : null;
  const wakeCanEnable = Boolean(wakeStatus?.model_downloaded || wakeStatus?.enabled);

  return (
    <div style={styles.container}>
      <div className="system-text" style={{ marginBottom: 20, fontSize: 14, letterSpacing: 4 }}>
        SETTINGS
      </div>

      <div style={styles.grid}>
        {/* Left column */}
        <div style={styles.column}>
          <LocalAiSetup
            endpoints={endpoints}
            onEndpointAdded={(ep) => setEndpoints((prev) => [...prev, ep])}
            onModelsLoaded={(eid, models) => setEndpointModels((prev) => ({ ...prev, [eid]: models }))}
            onAddToChain={handleAddToChain}
          />
          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>LOCAL LLM ENDPOINTS</div>
            <div style={styles.hint}>Connect to Ollama, vLLM, or any OpenAI-compatible server</div>

            {endpoints.map((ep) => (
              <div key={ep.id} style={{ background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.1)", borderRadius: 6, padding: 10, marginBottom: 8 }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  <div>
                    <span style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 12, fontWeight: 600 }}>{ep.name}</span>
                    <span style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginLeft: 8 }}>{ep.backend_type.toUpperCase()}</span>
                  </div>
                  <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
                    <span style={{ width: 8, height: 8, borderRadius: "50%", background: ep.last_health_status ? "rgba(16, 185, 129, 0.7)" : "rgba(255, 100, 100, 0.7)" }} />
                  </div>
                </div>
                <div style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, fontFamily: "var(--font-mono)", marginTop: 4 }}>{ep.url}</div>
                {endpointHealth[ep.id] && (
                  <div style={{ color: "rgba(0, 180, 255, 0.5)", fontSize: 10, marginTop: 4 }}>
                    {endpointHealth[ep.id].reachable ? `${endpointHealth[ep.id].model_count} models, ${endpointHealth[ep.id].latency_ms}ms` : "Unreachable"}
                  </div>
                )}
                {endpointModels[ep.id] && endpointModels[ep.id].length > 0 && (
                  <div style={{ marginTop: 6 }}>
                    <div style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 9 }}>MODELS:</div>
                    {endpointModels[ep.id].map((m) => (
                      <div key={m.id} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginTop: 2 }}>
                        <span style={{ color: "rgba(0, 180, 255, 0.6)", fontSize: 10, fontFamily: "var(--font-mono)" }}>{m.id}</span>
                        <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
                          <span style={{ color: "rgba(0, 180, 255, 0.3)", fontSize: 9 }}>{m.supports_tools === "native" ? "tools" : m.supports_tools === "prompt_injected" ? "prompt-tools" : "chat"}</span>
                          <button onClick={() => handleAddToChain("local", ep.id, m.id)} style={{ ...styles.saveBtn, fontSize: 9, padding: "2px 6px" }}>+ CHAIN</button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
                <div style={{ display: "flex", gap: 6, marginTop: 8 }}>
                  <button onClick={() => handleTestEndpoint(ep.id)} style={{ ...styles.saveBtn, fontSize: 9 }}>TEST</button>
                  <button onClick={() => handleRemoveEndpoint(ep.id)} style={{ ...styles.saveBtn, fontSize: 9, borderColor: "rgba(255, 100, 100, 0.3)", color: "rgba(255, 100, 100, 0.7)" }}>REMOVE</button>
                </div>
              </div>
            ))}

            <div style={{ display: "flex", gap: 6, marginTop: 8, flexDirection: "column" as const }}>
              <input value={newEndpointName} onChange={(e) => setNewEndpointName(e.target.value)} placeholder="Name (optional)" style={styles.tokenInput} />
              <div style={{ display: "flex", gap: 6 }}>
                <input value={newEndpointUrl} onChange={(e) => setNewEndpointUrl(e.target.value)} placeholder="http://localhost:11434" style={{ ...styles.tokenInput, flex: 1 }} />
                <button onClick={handleAddEndpoint} disabled={endpointBusy} style={styles.saveBtn}>{endpointBusy ? "..." : "ADD"}</button>
              </div>
            </div>
            {llmError && <div style={styles.errorText}>{llmError}</div>}
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>PROVIDER CHAIN</div>
            <div style={styles.hint}>Priority order for AI providers (drag or use arrows to reorder)</div>

            {chain.map((entry, i) => {
              const label = entry.provider_type === "local"
                ? `${entry.model_id || "unknown"} (local)`
                : entry.provider_type.charAt(0).toUpperCase() + entry.provider_type.slice(1);
              return (
                <div key={i} style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6, opacity: entry.enabled ? 1 : 0.4 }}>
                  <span style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, fontFamily: "var(--font-mono)", width: 16 }}>{i + 1}.</span>
                  <span style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 11, flex: 1, fontFamily: "var(--font-mono)" }}>{label}</span>
                  <button onClick={() => handleMoveChainEntry(i, "up")} disabled={i === 0} style={{ ...styles.saveBtn, fontSize: 9, padding: "1px 5px" }}>^</button>
                  <button onClick={() => handleMoveChainEntry(i, "down")} disabled={i === chain.length - 1} style={{ ...styles.saveBtn, fontSize: 9, padding: "1px 5px" }}>v</button>
                  <label style={{ cursor: "pointer" }}>
                    <input type="checkbox" checked={entry.enabled} onChange={() => handleToggleChainEntry(i)} style={styles.radio} />
                  </label>
                  <button onClick={() => handleRemoveFromChain(i)} style={{ ...styles.saveBtn, fontSize: 9, padding: "1px 5px", borderColor: "rgba(255, 100, 100, 0.3)", color: "rgba(255, 100, 100, 0.7)" }}>x</button>
                </div>
              );
            })}

            {chain.length === 0 && (
              <div style={{ color: "rgba(0, 180, 255, 0.3)", fontSize: 11, marginBottom: 8 }}>No providers configured. Add cloud or local providers below.</div>
            )}

            <div style={{ display: "flex", gap: 6, marginTop: 8 }}>
              <button onClick={() => handleAddToChain("claude")} style={styles.saveBtn}>+ Claude</button>
              <button onClick={() => handleAddToChain("openai")} style={styles.saveBtn}>+ OpenAI</button>
            </div>
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>AI PROVIDER (LEGACY)</div>
            <div style={styles.hint}>Used when no provider chain is configured</div>
            {options.map((opt) => (
              <label key={opt.value} style={styles.option}>
                <input type="radio" name="ai_provider" value={opt.value} checked={aiProvider === opt.value} onChange={() => handleProviderChange(opt.value)} style={styles.radio} />
                <span style={styles.optionLabel}>{opt.label}</span>
              </label>
            ))}
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>API KEYS</div>
            <div style={styles.hint}>Set in .env file at project root:</div>
            <code style={styles.code}>ANTHROPIC_API_KEY</code>
            <code style={styles.code}>OPENAI_API_KEY</code>
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>GOOGLE SERVICES</div>
            <div style={styles.hint}>Connect Gmail and Google Calendar</div>
            {googleConnected ? (
              <div style={{ color: "rgba(16, 185, 129, 0.7)", fontSize: 12 }}>Connected</div>
            ) : (
              <>
                <button onClick={handleGoogleConnect} disabled={connecting} style={styles.actionBtn}>
                  {connecting ? "CONNECTING..." : "CONNECT GOOGLE"}
                </button>
                {googleError && <div style={styles.errorText}>{googleError}</div>}
              </>
            )}
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>NOTION</div>
            <div style={styles.hint}>Enter your Notion integration token</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" value={notionToken} onChange={(e) => { setNotionToken(e.target.value); setNotionSaved(false); }} placeholder="ntn_..." style={styles.tokenInput} />
              <button onClick={handleSaveNotion} style={styles.saveBtn}>{notionSaved ? "SAVED" : "SAVE"}</button>
            </div>
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>GITHUB</div>
            <div style={styles.hint}>Enter your GitHub personal access token</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" value={githubToken} onChange={(e) => { setGithubToken(e.target.value); setGithubSaved(false); }} placeholder="ghp_..." style={styles.tokenInput} />
              <button onClick={handleSaveGitHub} style={styles.saveBtn}>{githubSaved ? "SAVED" : "SAVE"}</button>
            </div>
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>OBSIDIAN</div>
            <div style={styles.hint}>Connect to Obsidian vault via Local REST API plugin</div>
            <div style={{ display: "flex", gap: 8 }}>
              <input type="password" value={obsidianKey} onChange={(e) => { setObsidianKey(e.target.value); setObsidianSaved(false); }} placeholder="API key from Obsidian REST API plugin" style={styles.tokenInput} />
              <button onClick={handleSaveObsidian} style={styles.saveBtn}>{obsidianSaved ? "SAVED" : "SAVE"}</button>
            </div>
            <div style={{ color: "rgba(0, 180, 255, 0.3)", fontSize: 9, marginTop: 6 }}>
              Install "Local REST API" plugin in Obsidian, enable it, copy the API key
            </div>
          </div>
        </div>

        {/* Right column */}
        <div style={styles.column}>
          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>VOICE</div>
            <div style={styles.hint}>Cmd+Shift+J to start/stop voice input</div>
            <label style={styles.option}>
              <input type="checkbox" checked={voiceSettings?.tts_enabled ?? true} onChange={(e) => { void setVoiceSetting("tts_enabled", String(e.target.checked)); setVoiceSettingsState((prev) => prev ? { ...prev, tts_enabled: e.target.checked } : prev); }} style={styles.radio} />
              <span style={styles.optionLabel}>Enable text-to-speech</span>
            </label>
            {ttsVoices.length > 0 && (
              <div style={{ marginTop: 8 }}>
                <div style={{ color: "rgba(0, 180, 255, 0.5)", fontSize: 10, marginBottom: 4 }}>TTS Voice</div>
                <select value={voiceSettings?.tts_voice ?? "Samantha"} onChange={(e) => { void setVoiceSetting("tts_voice", e.target.value); setVoiceSettingsState((prev) => prev ? { ...prev, tts_voice: e.target.value } : prev); }} style={styles.select}>
                  {ttsVoices.map((voice) => <option key={voice} value={voice}>{voice}</option>)}
                </select>
              </div>
            )}
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>WAKE WORD</div>
            <div style={styles.hint}>Always-on local wake-word detection for "Hey Jarvis"</div>
            <label style={styles.option}>
              <input type="checkbox" checked={wakeStatus?.enabled ?? false} disabled={!wakeCanEnable || wakeBusy || modelDownloading !== null} onChange={(e) => { void handleWakeToggle(e.target.checked); }} style={styles.radio} />
              <span style={styles.optionLabel}>Enable wake word</span>
            </label>
            <div style={styles.statusText}>{getWakeStatusLine(voiceState)}</div>
            <button onClick={() => { void handleDownloadModel(); }} disabled={wakeBusy || modelDownloading !== null || wakeStatus?.model_downloaded} style={styles.actionBtn}>
              {modelDownloading !== null ? "DOWNLOADING..." : wakeStatus?.model_downloaded ? "MODEL READY" : "DOWNLOAD MODEL"}
            </button>
            {modelDownloading !== null && (
              <div style={styles.progressShell}>
                <div style={{ ...styles.progressFill, width: `${modelDownloading}%` }} />
              </div>
            )}
            <div style={styles.privacyText}>
              Wake-word audio is processed locally for detection. After activation, full commands use cloud speech-to-text first when OPENAI_API_KEY is configured, then fall back to local Whisper.
            </div>
            {wakeError && <div style={styles.errorText}>{wakeError}</div>}
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>ASSISTANT</div>
            <label style={styles.option}>
              <input type="checkbox" defaultChecked={true} onChange={(e) => void updateSetting("auto_briefing", String(e.target.checked))} style={styles.radio} />
              <span style={styles.optionLabel}>Speak morning briefing on startup (once per day)</span>
            </label>
          </div>

          <div className="panel" style={styles.panel}>
            <div className="label" style={styles.sectionTitle}>WALLPAPER MODE</div>
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
            <div style={styles.privacyText}>
              When active, JARVIS goes fullscreen behind all windows.
              After enabling, it stays interactive until you send it to the background.
              To bring it back later, say "Hey Jarvis", click the tray icon, or press Cmd+Shift+W.
              Closing the window sends it back to the background.
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 24, paddingBottom: 48 },
  grid: { display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, alignItems: "start" },
  column: { display: "flex", flexDirection: "column" as const, gap: 12 },
  panel: { padding: 16 },
  sectionTitle: { marginBottom: 12 },
  option: { display: "flex", alignItems: "center", gap: 8, marginBottom: 10, cursor: "pointer" },
  radio: { accentColor: "rgba(0, 180, 255, 0.8)" },
  optionLabel: { color: "rgba(0, 180, 255, 0.7)", fontSize: 13 },
  hint: { color: "rgba(0, 180, 255, 0.5)", fontSize: 12, marginBottom: 8 },
  code: { display: "block", color: "rgba(0, 180, 255, 0.6)", fontFamily: "var(--font-mono)", fontSize: 11, background: "rgba(0, 180, 255, 0.04)", padding: "4px 8px", borderRadius: 4, marginBottom: 4 },
  tokenInput: { flex: 1, background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "6px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)", outline: "none" },
  saveBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 10 },
  actionBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "8px 16px", color: "rgba(0, 180, 255, 0.8)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 11 },
  select: { background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "4px 8px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)" },
  statusText: { color: "rgba(255, 180, 0, 0.85)", fontSize: 11, fontFamily: "var(--font-mono)", marginBottom: 10 },
  progressShell: { marginTop: 12, width: "100%", height: 8, borderRadius: 999, overflow: "hidden", background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.15)" },
  progressFill: { height: "100%", background: "linear-gradient(90deg, rgba(0, 180, 255, 0.65), rgba(96, 165, 250, 0.95))", transition: "width 180ms ease-out" },
  privacyText: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 12, lineHeight: 1.5 },
  errorText: { color: "rgba(255, 100, 100, 0.85)", fontSize: 11, marginTop: 10 },
};
