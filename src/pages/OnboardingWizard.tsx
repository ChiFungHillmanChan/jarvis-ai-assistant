import { useState } from "react";
import LocalAiSetup from "../components/LocalAiSetup";
import { googleConnect, markOnboardingComplete, updateSetting } from "../lib/commands";

type Step = 1 | 2 | 3 | 4;

interface Props {
  onComplete: () => void;
}

export default function OnboardingWizard({ onComplete }: Props) {
  const [step, setStep] = useState<Step>(1);
  const [openaiKey, setOpenaiKey] = useState("");
  const [claudeKey, setClaudeKey] = useState("");
  const [geminiKey, setGeminiKey] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function saveByokKeys() {
    if (openaiKey.trim()) {
      await updateSetting("openai_api_key", openaiKey.trim());
    }
    if (claudeKey.trim()) {
      await updateSetting("claude_api_key", claudeKey.trim());
    }
    if (geminiKey.trim()) {
      await updateSetting("gemini_api_key", geminiKey.trim());
    }
  }

  async function handleFinish() {
    setError(null);
    try {
      await saveByokKeys();
      await markOnboardingComplete();
      onComplete();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div style={styles.container}>
      <div style={styles.stepBadge}>STEP {step} / 4</div>
      <h1 style={styles.title}>{titleFor(step)}</h1>
      {error && <div style={styles.error}>{error}</div>}

      {step === 1 && (
        <div>
          <p style={styles.body}>
            JARVIS runs an on-device model by default for privacy. Pick one below
            (skip if you only want cloud providers).
          </p>
          <LocalAiSetup
            endpoints={[]}
            onEndpointAdded={() => {}}
            onModelsLoaded={() => {}}
            onAddToChain={() => {}}
          />
          <div style={styles.row}>
            <button style={styles.skip} onClick={() => setStep(2)}>Skip</button>
            <button style={styles.next} onClick={() => setStep(2)}>Next</button>
          </div>
        </div>
      )}

      {step === 2 && (
        <div>
          <p style={styles.body}>
            Optional: paste any cloud provider keys you already have. All optional --
            you can skip and add them later in Settings.
          </p>
          <input type="password" style={styles.input} placeholder="OpenAI API key" value={openaiKey} onChange={(e) => setOpenaiKey(e.target.value)} />
          <input type="password" style={styles.input} placeholder="Claude API key" value={claudeKey} onChange={(e) => setClaudeKey(e.target.value)} />
          <input type="password" style={styles.input} placeholder="Gemini API key" value={geminiKey} onChange={(e) => setGeminiKey(e.target.value)} />
          <div style={styles.row}>
            <button style={styles.skip} onClick={() => setStep(3)}>Skip</button>
            <button style={styles.next} onClick={() => setStep(3)}>Next</button>
          </div>
        </div>
      )}

      {step === 3 && (
        <div>
          <p style={styles.body}>
            Connect Gmail and Calendar so JARVIS can triage email and schedule
            follow-ups. You can skip and connect later in Settings.
          </p>
          <button style={styles.next} onClick={async () => {
            setError(null);
            try {
              await googleConnect();
              setStep(4);
            } catch (e) {
              setError(String(e));
            }
          }}>Connect Google</button>
          <button style={styles.skip} onClick={() => {
            setError(null);
            setStep(4);
          }}>Skip</button>
        </div>
      )}

      {step === 4 && (
        <div>
          <p style={styles.body}>
            Setup complete. Press Start to open the Dashboard.
          </p>
          <button style={styles.next} onClick={handleFinish}>Start</button>
        </div>
      )}
    </div>
  );
}

function titleFor(step: Step): string {
  switch (step) {
    case 1: return "Set up your local model";
    case 2: return "Cloud providers (optional)";
    case 3: return "Connect Google";
    case 4: return "Ready";
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
  error: { color: "rgba(255,100,100,0.9)", border: "1px solid rgba(255,100,100,0.3)", borderRadius: 6, padding: 10, marginBottom: 12, fontSize: 12 },
};
