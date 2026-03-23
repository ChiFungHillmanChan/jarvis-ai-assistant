import type { VoiceState } from "../lib/types";

interface VoiceIndicatorProps { state: VoiceState; onStop: () => void; }

export default function VoiceIndicator({ state, onStop }: VoiceIndicatorProps) {
  if (state === "Idle" || state === "Disabled") return null;

  const label = state === "Listening" ? "LISTENING..."
    : state === "Processing" ? "PROCESSING..."
    : state === "Speaking" ? "SPEAKING..."
    : typeof state === "object" && "Error" in state ? `ERROR: ${state.Error}`
    : "";

  const color = state === "Listening" ? "rgba(0, 180, 255, 0.9)"
    : state === "Processing" ? "rgba(255, 180, 0, 0.8)"
    : state === "Speaking" ? "rgba(16, 185, 129, 0.8)"
    : "rgba(255, 100, 100, 0.8)";

  return (
    <div style={styles.overlay} onClick={state === "Listening" ? onStop : undefined}>
      <div style={{ ...styles.indicator, borderColor: color }}>
        <div style={{ ...styles.dot, background: color }} className="animate-glow" />
        <span style={{ ...styles.label, color }}>{label}</span>
        {state === "Listening" && <span style={styles.hint}>Click or press Cmd+Shift+J to stop</span>}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  overlay: { position: "fixed", bottom: 24, left: "50%", transform: "translateX(-50%)", zIndex: 200, cursor: "pointer" },
  indicator: { display: "flex", alignItems: "center", gap: 10, padding: "10px 20px", borderRadius: 24, border: "1px solid", background: "rgba(10, 14, 26, 0.95)" },
  dot: { width: 10, height: 10, borderRadius: "50%" },
  label: { fontFamily: "var(--font-mono)", fontSize: 11, letterSpacing: 1.5 },
  hint: { color: "rgba(0, 180, 255, 0.3)", fontSize: 9, fontFamily: "var(--font-mono)" },
};
