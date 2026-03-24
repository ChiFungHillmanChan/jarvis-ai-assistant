import type { VoiceState } from "../lib/types";

interface VoiceIndicatorProps { state: VoiceState; onStop: () => void; }

export default function VoiceIndicator({ state, onStop }: VoiceIndicatorProps) {
  if (state === "Idle" || state === "Disabled") return null;

  const isManualListening = state === "Listening";
  const label = state === "Listening" ? "LISTENING..."
    : state === "Processing" ? "PROCESSING..."
    : state === "Speaking" ? "SPEAKING..."
    : state === "WakeWordListening" ? "WAKE WORD ACTIVE"
    : state === "WakeWordDetected" ? "WAKE WORD DETECTED"
    : state === "WakeWordProcessing" ? "PROCESSING WAKE REQUEST..."
    : state === "WakeWordSpeaking" ? "JARVIS RESPONDING..."
    : typeof state === "object" && "ModelDownloading" in state ? `DOWNLOADING MODEL... ${state.ModelDownloading}%`
    : typeof state === "object" && "Error" in state ? `ERROR: ${state.Error}`
    : "";

  const color = state === "Listening" || state === "WakeWordListening" ? "rgba(0, 180, 255, 0.9)"
    : state === "Processing" || state === "WakeWordDetected" || state === "WakeWordProcessing" ? "rgba(255, 180, 0, 0.85)"
    : state === "Speaking" || state === "WakeWordSpeaking" ? "rgba(16, 185, 129, 0.85)"
    : typeof state === "object" && "ModelDownloading" in state ? "rgba(96, 165, 250, 0.85)"
    : "rgba(255, 100, 100, 0.8)";

  return (
    <div style={{ ...styles.overlay, cursor: isManualListening ? "pointer" : "default" }} onClick={isManualListening ? onStop : undefined}>
      <div style={{ ...styles.indicator, borderColor: color }}>
        <div style={{ ...styles.dot, background: color }} className="animate-glow" />
        <span style={{ ...styles.label, color }}>{label}</span>
        {isManualListening && <span style={styles.hint}>Click or press Cmd+Shift+J to stop</span>}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  overlay: { position: "fixed", bottom: 24, left: "50%", transform: "translateX(-50%)", zIndex: 200 },
  indicator: { display: "flex", alignItems: "center", gap: 10, padding: "10px 20px", borderRadius: 24, border: "1px solid", background: "rgba(10, 14, 26, 0.95)" },
  dot: { width: 10, height: 10, borderRadius: "50%" },
  label: { fontFamily: "var(--font-mono)", fontSize: 11, letterSpacing: 1.5 },
  hint: { color: "rgba(0, 180, 255, 0.3)", fontSize: 9, fontFamily: "var(--font-mono)" },
};
