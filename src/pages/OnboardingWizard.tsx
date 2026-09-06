import { useState } from "react";
import {
  googleConnect,
  markOnboardingComplete,
  openUrl,
  setApiKey,
} from "../lib/commands";

type Step = 1 | 2 | 3;
type OauthErrorKind = "denied" | "timeout" | "no-creds" | "generic";

interface Props {
  onComplete: () => void;
}

// Public legal pages. Update if the marketing domain changes.
const LEGAL_BASE_URL = "https://jarvis-automation.com";
const PRIVACY_URL = `${LEGAL_BASE_URL}/privacy`;
const TERMS_URL = `${LEGAL_BASE_URL}/terms`;

function classifyOauthError(message: string): OauthErrorKind {
  const m = message.toLowerCase();
  if (m.includes("not configured")) return "no-creds";
  if (m.includes("timeout") || m.includes("did not respond")) return "timeout";
  if (m.includes("denied") || m.includes("access_denied")) return "denied";
  return "generic";
}

function describeOauthError(kind: OauthErrorKind, raw: string): string {
  switch (kind) {
    case "no-creds":
      return "Google sign-in is not available in this build. Please contact support.";
    case "timeout":
      return `Google did not send JARVIS a sign-in response. If Google showed "Access blocked" or "app not verified", your account must be added as a tester first. Otherwise check your connection and try again.`;
    case "denied":
      return "Permission was declined in the browser. JARVIS needs Gmail and Calendar access to triage email — try again, or skip and connect later in Settings.";
    default:
      return raw;
  }
}

export default function OnboardingWizard({ onComplete }: Props) {
  const [step, setStep] = useState<Step>(1);
  const [openaiKey, setOpenaiKey] = useState("");
  const [claudeKey, setClaudeKey] = useState("");
  const [geminiKey, setGeminiKey] = useState("");

  const [oauthInFlight, setOauthInFlight] = useState(false);
  const [oauthError, setOauthError] = useState<{ kind: OauthErrorKind; raw: string } | null>(null);

  const [globalError, setGlobalError] = useState<string | null>(null);

  async function saveByokKeys() {
    if (geminiKey.trim()) await setApiKey("gemini", geminiKey.trim());
    if (openaiKey.trim()) await setApiKey("openai", openaiKey.trim());
    if (claudeKey.trim()) await setApiKey("claude", claudeKey.trim());
  }

  async function handleStep1Next() {
    setGlobalError(null);
    try {
      await saveByokKeys();
      setStep(2);
    } catch (e) {
      setGlobalError(String(e));
    }
  }

  async function handleConnectGoogle() {
    setOauthError(null);
    setOauthInFlight(true);
    try {
      await googleConnect();
      setOauthInFlight(false);
      setStep(3);
    } catch (e) {
      const raw = String(e);
      setOauthError({ kind: classifyOauthError(raw), raw });
      setOauthInFlight(false);
    }
  }

  async function handleFinish() {
    setGlobalError(null);
    try {
      await markOnboardingComplete();
      onComplete();
    } catch (e) {
      setGlobalError(String(e));
    }
  }

  return (
    <div style={styles.container}>
      <div style={styles.stepBadge}>STEP {step} / 3</div>
      <h1 style={styles.title}>{titleFor(step)}</h1>
      {globalError && <div style={styles.error}>{globalError}</div>}

      {step === 1 && (
        <div>
          <p style={styles.body}>
            Paste any cloud provider keys you already have. A Gemini key is the
            default. Claude or OpenAI keys also work, and JARVIS falls back
            between whichever you provide. All optional — you can skip and add
            them later in Settings.
          </p>
          <input
            type="password"
            style={styles.input}
            placeholder="Gemini API key (recommended for fastest replies)"
            value={geminiKey}
            onChange={(e) => setGeminiKey(e.target.value)}
          />
          <input
            type="password"
            style={styles.input}
            placeholder="OpenAI API key"
            value={openaiKey}
            onChange={(e) => setOpenaiKey(e.target.value)}
          />
          <input
            type="password"
            style={styles.input}
            placeholder="Claude API key"
            value={claudeKey}
            onChange={(e) => setClaudeKey(e.target.value)}
          />
          <div style={styles.row}>
            <button style={styles.skip} onClick={() => setStep(2)}>Skip</button>
            <button style={styles.next} onClick={handleStep1Next}>Next</button>
          </div>
        </div>
      )}

      {step === 2 && (
        <div>
          <p style={styles.body}>
            Connect Gmail and Calendar so JARVIS can triage email and schedule
            follow-ups. You can skip and connect later in Settings.
          </p>
          <div style={styles.scopeBox}>
            <div style={styles.scopeTitle}>JARVIS will request access to:</div>
            <ul style={styles.scopeList}>
              <li>Read your Gmail messages and labels</li>
              <li>Send, archive, and label messages on your behalf</li>
              <li>Read and create Calendar events</li>
            </ul>
            <div style={styles.scopeNote}>
              Email and calendar content is processed by the AI provider you choose
              (local model, OpenAI, Anthropic, Gemini, or AWS Bedrock). JARVIS does
              not store a copy on its servers.
            </div>
          </div>
          <div style={styles.legalNotice}>
            By continuing, you agree to the{" "}
            <a
              href={PRIVACY_URL}
              onClick={(e) => { e.preventDefault(); openUrl(PRIVACY_URL).catch(() => {}); }}
              style={styles.link}
            >Privacy Policy</a>
            {" "}and{" "}
            <a
              href={TERMS_URL}
              onClick={(e) => { e.preventDefault(); openUrl(TERMS_URL).catch(() => {}); }}
              style={styles.link}
            >Terms of Service</a>.
          </div>

          {oauthError && (
            <div style={styles.error}>
              <div style={styles.errorTitle}>Couldn't connect to Google</div>
              <div style={styles.errorBody}>{describeOauthError(oauthError.kind, oauthError.raw)}</div>
              <div style={styles.errorRow}>
                {oauthError.kind !== "no-creds" && (
                  <button style={styles.errorRetry} onClick={handleConnectGoogle}>Try again</button>
                )}
                <button
                  style={styles.errorSkip}
                  onClick={() => { setOauthError(null); setStep(3); }}
                >Skip and connect later</button>
              </div>
            </div>
          )}

          {oauthInFlight && (
            <div style={styles.statusBox}>
              <div className="animate-glow" style={styles.statusDot} />
              <div style={styles.statusText}>
                Opening Google in your browser. Approve the permissions, then return here.
              </div>
              <button
                style={styles.statusSkip}
                onClick={() => { setOauthInFlight(false); setStep(3); }}
              >Skip for now</button>
            </div>
          )}

          {!oauthInFlight && !oauthError && (
            <div style={styles.row}>
              <button style={styles.skip} onClick={() => setStep(3)}>Skip</button>
              <button style={styles.next} onClick={handleConnectGoogle}>Connect Google</button>
            </div>
          )}
        </div>
      )}

      {step === 3 && (
        <div>
          <p style={styles.body}>
            Setup complete. Press Start to open the Dashboard. You can run a test
            message from the chat once it loads — if your AI provider is unreachable,
            the chat will surface a clear error and you can update keys in Settings.
          </p>
          <div style={styles.row}>
            <button style={styles.next} onClick={handleFinish}>Start</button>
          </div>
        </div>
      )}
    </div>
  );
}

function titleFor(step: Step): string {
  switch (step) {
    case 1: return "Cloud AI providers";
    case 2: return "Connect Google";
    case 3: return "Ready";
  }
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 32, maxWidth: 640, margin: "0 auto" },
  stepBadge: { fontFamily: "var(--font-mono)", fontSize: 11, color: "rgba(0,180,255,0.55)", letterSpacing: 4 },
  title: { color: "rgba(0,180,255,0.9)", fontSize: 24, marginTop: 8, marginBottom: 20 },
  body: { color: "rgba(0,180,255,0.7)", fontSize: 14, lineHeight: 1.6, marginBottom: 16 },
  input: { display: "block", width: "100%", marginBottom: 8, background: "rgba(0,180,255,0.03)", border: "1px solid rgba(0,180,255,0.15)", borderRadius: 6, padding: "8px 10px", color: "rgba(0,180,255,0.8)", fontFamily: "var(--font-mono)", fontSize: 12 },
  row: { display: "flex", gap: 8, marginTop: 16 },
  next: { background: "rgba(0,180,255,0.12)", border: "1px solid rgba(0,180,255,0.4)", borderRadius: 6, padding: "10px 14px", color: "rgba(0,180,255,0.9)", cursor: "pointer", fontFamily: "var(--font-mono)" },
  skip: { background: "transparent", border: "1px solid rgba(0,180,255,0.2)", borderRadius: 6, padding: "10px 14px", color: "rgba(0,180,255,0.6)", cursor: "pointer", fontFamily: "var(--font-mono)" },
  error: { color: "rgba(255,100,100,0.9)", border: "1px solid rgba(255,100,100,0.3)", borderRadius: 6, padding: 12, marginBottom: 12, fontSize: 12, background: "rgba(255,100,100,0.04)" },
  errorTitle: { fontWeight: 600, marginBottom: 4, fontSize: 12, letterSpacing: 0.5 },
  errorBody: { color: "rgba(255,180,180,0.85)", lineHeight: 1.5 },
  errorRow: { display: "flex", gap: 8, marginTop: 10 },
  errorRetry: { background: "rgba(255,100,100,0.12)", border: "1px solid rgba(255,100,100,0.4)", borderRadius: 6, padding: "8px 12px", color: "rgba(255,180,180,0.95)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 11 },
  errorSkip: { background: "transparent", border: "1px solid rgba(255,100,100,0.25)", borderRadius: 6, padding: "8px 12px", color: "rgba(255,180,180,0.7)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 11 },
  scopeBox: { padding: "12px 14px", marginBottom: 12, background: "rgba(0,180,255,0.04)", border: "1px solid rgba(0,180,255,0.18)", borderRadius: 8 },
  scopeTitle: { color: "rgba(0,180,255,0.85)", fontSize: 12, fontWeight: 600, marginBottom: 6, fontFamily: "var(--font-mono)", letterSpacing: 1 },
  scopeList: { color: "rgba(0,180,255,0.7)", fontSize: 12, lineHeight: 1.7, paddingLeft: 18, margin: 0 },
  scopeNote: { color: "rgba(0,180,255,0.55)", fontSize: 11, lineHeight: 1.5, marginTop: 8, paddingTop: 8, borderTop: "1px solid rgba(0,180,255,0.12)" },
  legalNotice: { color: "rgba(0,180,255,0.6)", fontSize: 11, lineHeight: 1.6, marginBottom: 14 },
  link: { color: "rgba(0,180,255,0.9)", textDecoration: "underline", cursor: "pointer" },
  statusBox: { display: "flex", alignItems: "center", gap: 12, padding: "14px 16px", marginBottom: 12, background: "rgba(0,180,255,0.04)", border: "1px solid rgba(0,180,255,0.25)", borderRadius: 8 },
  statusDot: { width: 10, height: 10, borderRadius: "50%", background: "rgba(0,180,255,0.8)", boxShadow: "0 0 8px rgba(0,180,255,0.6)", flexShrink: 0 },
  statusText: { color: "rgba(0,180,255,0.85)", fontSize: 12, lineHeight: 1.5 },
  statusSkip: { marginLeft: "auto", flexShrink: 0, background: "transparent", border: "1px solid rgba(0,180,255,0.2)", borderRadius: 6, padding: "8px 12px", color: "rgba(0,180,255,0.6)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 11, whiteSpace: "nowrap" },
  testSuccessBox: { padding: "12px 14px", marginBottom: 12, background: "rgba(80,255,180,0.05)", border: "1px solid rgba(80,255,180,0.35)", borderRadius: 8 },
  testSuccessTitle: { color: "rgba(80,255,180,0.9)", fontSize: 12, fontWeight: 600, fontFamily: "var(--font-mono)", letterSpacing: 1, marginBottom: 6 },
  testResponseText: { color: "rgba(180,255,220,0.85)", fontSize: 12, lineHeight: 1.5, fontFamily: "var(--font-mono)" },
};
