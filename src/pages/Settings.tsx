import { useEffect, useState } from "react";
import { getSettings, updateSetting, googleConnect, googleStatus, saveNotionToken, saveGitHubToken, saveObsidianKey, getVoiceSettings, setVoiceSetting, listTtsVoices } from "../lib/commands";
import type { VoiceSettings } from "../lib/types";

export default function Settings() {
  const [aiProvider, setAiProvider] = useState("claude_primary");
  const [loaded, setLoaded] = useState(false);
  const [googleConnected, setGoogleConnected] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [notionToken, setNotionToken] = useState("");
  const [githubToken, setGithubToken] = useState("");
  const [notionSaved, setNotionSaved] = useState(false);
  const [githubSaved, setGithubSaved] = useState(false);
  const [obsidianKey, setObsidianKey] = useState("");
  const [obsidianSaved, setObsidianSaved] = useState(false);
  const [voiceSettings, setVoiceSettingsState] = useState<VoiceSettings | null>(null);
  const [ttsVoices, setTtsVoices] = useState<string[]>([]);

  useEffect(() => { getSettings().then((s) => { setAiProvider(s.values["ai_provider"] || "claude_primary"); setLoaded(true); }); }, []);
  useEffect(() => { googleStatus().then(setGoogleConnected); }, []);
  useEffect(() => { getVoiceSettings().then(setVoiceSettingsState); listTtsVoices().then(setTtsVoices); }, []);

  function handleProviderChange(value: string) { setAiProvider(value); updateSetting("ai_provider", value); }
  async function handleGoogleConnect() { setConnecting(true); try { await googleConnect(); setGoogleConnected(true); } catch (e) { console.error(e); } finally { setConnecting(false); } }
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

  if (!loaded) return (<div style={{ padding: 24 }}><div className="system-text animate-glow">LOADING SETTINGS...</div></div>);

  const options = [
    { value: "claude_primary", label: "Claude (primary) + OpenAI (fallback)" },
    { value: "openai_primary", label: "OpenAI (primary) + Claude (fallback)" },
    { value: "claude_only", label: "Claude only" },
    { value: "openai_only", label: "OpenAI only" },
  ];

  return (
    <div style={styles.container}>
      <div className="system-text" style={{ marginBottom: 24 }}>SETTINGS</div>
      <div className="panel" style={{ maxWidth: 500, padding: 16 }}>
        <div className="label" style={{ marginBottom: 12 }}>AI PROVIDER</div>
        {options.map((opt) => (
          <label key={opt.value} style={styles.option}>
            <input type="radio" name="ai_provider" value={opt.value} checked={aiProvider === opt.value} onChange={() => handleProviderChange(opt.value)} style={styles.radio} />
            <span style={styles.optionLabel}>{opt.label}</span>
          </label>
        ))}
      </div>
      <div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 12 }}>API KEYS</div>
        <div style={styles.hint}>Set in .env file at project root:</div>
        <code style={styles.code}>ANTHROPIC_API_KEY</code>
        <code style={styles.code}>OPENAI_API_KEY</code>
      </div>
      <div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 12 }}>GOOGLE SERVICES</div>
        <div style={styles.hint}>Connect Gmail and Google Calendar</div>
        {googleConnected ? (
          <div style={{ color: "rgba(16, 185, 129, 0.7)", fontSize: 12 }}>Connected</div>
        ) : (
          <button onClick={handleGoogleConnect} disabled={connecting} style={{ background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "8px 16px", color: "rgba(0, 180, 255, 0.8)", cursor: connecting ? "wait" : "pointer", fontFamily: "var(--font-mono)", fontSize: 11 }}>
            {connecting ? "CONNECTING..." : "CONNECT GOOGLE"}
          </button>
        )}
      </div>
      <div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 12 }}>NOTION</div>
        <div style={styles.hint}>Enter your Notion integration token</div>
        <div style={{ display: "flex", gap: 8 }}>
          <input type="password" value={notionToken} onChange={(e) => { setNotionToken(e.target.value); setNotionSaved(false); }}
            placeholder="ntn_..." style={styles.tokenInput} />
          <button onClick={handleSaveNotion} style={styles.saveBtn}>{notionSaved ? "SAVED" : "SAVE"}</button>
        </div>
      </div>
      <div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 12 }}>GITHUB</div>
        <div style={styles.hint}>Enter your GitHub personal access token</div>
        <div style={{ display: "flex", gap: 8 }}>
          <input type="password" value={githubToken} onChange={(e) => { setGithubToken(e.target.value); setGithubSaved(false); }}
            placeholder="ghp_..." style={styles.tokenInput} />
          <button onClick={handleSaveGitHub} style={styles.saveBtn}>{githubSaved ? "SAVED" : "SAVE"}</button>
        </div>
      </div>
      <div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 12 }}>OBSIDIAN</div>
        <div style={styles.hint}>Connect to Obsidian vault via Local REST API plugin</div>
        <div style={{ display: "flex", gap: 8 }}>
          <input type="password" value={obsidianKey} onChange={(e) => { setObsidianKey(e.target.value); setObsidianSaved(false); }}
            placeholder="API key from Obsidian REST API plugin" style={styles.tokenInput} />
          <button onClick={handleSaveObsidian} style={styles.saveBtn}>{obsidianSaved ? "SAVED" : "SAVE"}</button>
        </div>
        <div style={{ color: "rgba(0, 180, 255, 0.3)", fontSize: 9, marginTop: 6 }}>
          Install "Local REST API" plugin in Obsidian, enable it, copy the API key
        </div>
      </div>
      <div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 12 }}>VOICE</div>
        <div style={styles.hint}>Cmd+Shift+J to start/stop voice input</div>
        <label style={styles.option}>
          <input type="checkbox" checked={voiceSettings?.tts_enabled ?? true}
            onChange={(e) => { setVoiceSetting("tts_enabled", String(e.target.checked)); setVoiceSettingsState(prev => prev ? {...prev, tts_enabled: e.target.checked} : prev); }}
            style={styles.radio} />
          <span style={styles.optionLabel}>Enable text-to-speech</span>
        </label>
        {ttsVoices.length > 0 && (
          <div style={{ marginTop: 8 }}>
            <div style={{ color: "rgba(0, 180, 255, 0.5)", fontSize: 10, marginBottom: 4 }}>TTS Voice</div>
            <select value={voiceSettings?.tts_voice ?? "Samantha"}
              onChange={(e) => { setVoiceSetting("tts_voice", e.target.value); setVoiceSettingsState(prev => prev ? {...prev, tts_voice: e.target.value} : prev); }}
              style={{ background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "4px 8px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)" }}>
              {ttsVoices.map(v => <option key={v} value={v}>{v}</option>)}
            </select>
          </div>
        )}
      </div>
      <div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
        <div className="label" style={{ marginBottom: 12 }}>ASSISTANT</div>
        <label style={styles.option}>
          <input type="checkbox" defaultChecked={true}
            onChange={(e) => updateSetting("auto_briefing", String(e.target.checked))}
            style={styles.radio} />
          <span style={styles.optionLabel}>Speak morning briefing on startup (once per day)</span>
        </label>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 24, height: "100%", overflowY: "auto" },
  option: { display: "flex", alignItems: "center", gap: 8, marginBottom: 10, cursor: "pointer" },
  radio: { accentColor: "rgba(0, 180, 255, 0.8)" },
  optionLabel: { color: "rgba(0, 180, 255, 0.7)", fontSize: 13 },
  hint: { color: "rgba(0, 180, 255, 0.5)", fontSize: 12, marginBottom: 8 },
  code: { display: "block", color: "rgba(0, 180, 255, 0.6)", fontFamily: "var(--font-mono)", fontSize: 11, background: "rgba(0, 180, 255, 0.04)", padding: "4px 8px", borderRadius: 4, marginBottom: 4 },
  tokenInput: { flex: 1, background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "6px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)", outline: "none" },
  saveBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 10 },
};
