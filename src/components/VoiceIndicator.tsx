import { memo } from "react";
import type { VoiceState } from "../lib/types";

interface Props { state: VoiceState; onStop: () => void; }

function getStateInfo(state: VoiceState): { label: string; color: string } | null {
  if (state === "Listening" || state === "WakeWordListening") return { label: "LISTENING", color: "rgba(0, 180, 255, 0.9)" };
  if (state === "Processing" || state === "WakeWordDetected" || state === "WakeWordProcessing") return { label: "PROCESSING", color: "rgba(255, 180, 0, 0.85)" };
  if (state === "Speaking" || state === "WakeWordSpeaking") return { label: "SPEAKING", color: "rgba(16, 185, 129, 0.85)" };
  if (typeof state === "object" && "ModelDownloading" in state) return { label: "DOWNLOADING", color: "rgba(96, 165, 250, 0.85)" };
  if (typeof state === "object" && "Error" in state) return { label: "ERROR", color: "rgba(255, 100, 100, 0.8)" };
  return null;
}

export default memo(function VoiceIndicator({ state, onStop }: Props) {
  const info = getStateInfo(state);
  if (!info) return null;

  return (
    <div onClick={onStop} style={styles.container} title="Cmd+Shift+J">
      <div style={{ ...styles.dot, background: info.color, boxShadow: `0 0 8px ${info.color.replace(/[\d.]+\)$/, "0.5)")}` }} className="animate-glow" />
      <span style={{ ...styles.label, color: info.color.replace(/[\d.]+\)$/, "0.6)") }}>{info.label}</span>
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  container: { position: "fixed", bottom: 16, left: "50%", transform: "translateX(-50%)", display: "flex", alignItems: "center", gap: 6, padding: "5px 12px", background: "rgba(10, 14, 26, 0.8)", borderRadius: 12, border: "1px solid rgba(0, 180, 255, 0.1)", cursor: "pointer", zIndex: 50 },
  dot: { width: 6, height: 6, borderRadius: "50%", flexShrink: 0 },
  label: { fontFamily: "var(--font-mono)", fontSize: 9, letterSpacing: 1 },
};
