import { memo } from "react";
import type { AssistantPhase } from "../lib/types";

interface AssistantHudProps {
  visible: boolean;
  phase: AssistantPhase;
  status: string | null;
  thinking: string;
  responsePreview: string;
}

const PHASE_LABELS: Record<AssistantPhase, string> = {
  idle: "STANDBY",
  listening: "LISTENING",
  transcribing: "TRANSCRIBING",
  thinking: "THINKING",
  planning: "PLANNING",
  acting: "ACTING",
  responding: "RESPONDING",
  speaking: "SPEAKING",
};

const PHASE_COLORS: Record<AssistantPhase, string> = {
  idle: "rgba(0, 180, 255, 0.45)",
  listening: "rgba(0, 180, 255, 0.85)",
  transcribing: "rgba(96, 165, 250, 0.85)",
  thinking: "rgba(255, 180, 0, 0.85)",
  planning: "rgba(255, 180, 0, 0.85)",
  acting: "rgba(255, 180, 0, 0.92)",
  responding: "rgba(0, 180, 255, 0.85)",
  speaking: "rgba(16, 185, 129, 0.85)",
};

function truncateTail(text: string, max = 220) {
  return text.length <= max ? text : `…${text.slice(-max)}`;
}

export default memo(function AssistantHud({
  visible,
  phase,
  status,
  thinking,
  responsePreview,
}: AssistantHudProps) {
  if (!visible) return null;

  const color = PHASE_COLORS[phase];
  const phaseLabel = PHASE_LABELS[phase];
  const trimmedThinking = truncateTail(thinking.trim(), 180);
  const trimmedResponse = truncateTail(responsePreview.trim(), 220);

  return (
    <div style={styles.shell} className="no-drag">
      <div style={styles.panel}>
        <div style={styles.header}>
          <div style={styles.phaseRow}>
            <span style={{ ...styles.dot, background: color, boxShadow: `0 0 14px ${color}` }} />
            <span className="system-text" style={{ ...styles.phase, color }}>
              {phaseLabel}
            </span>
          </div>
          <span className="system-text" style={styles.brand}>
            JARVIS LIVE
          </span>
        </div>

        {status && (
          <div style={styles.statusLine}>
            <span style={styles.statusLabel}>STATUS</span>
            <span style={styles.statusText}>{status}</span>
          </div>
        )}

        {trimmedThinking && phase !== "speaking" && (
          <div style={styles.traceBlock}>
            <div style={styles.traceLabel}>REASONING TRACE</div>
            <div style={styles.traceText}>{trimmedThinking}</div>
          </div>
        )}

        {trimmedResponse && (
          <div style={styles.traceBlock}>
            <div style={styles.traceLabel}>VOICE / RESPONSE</div>
            <div style={styles.responseText}>
              {trimmedResponse}
              {phase !== "idle" && <span style={styles.cursor}>|</span>}
            </div>
          </div>
        )}
      </div>
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  shell: {
    position: "fixed",
    left: "50%",
    bottom: 56,
    transform: "translateX(-50%)",
    width: "min(640px, calc(100vw - 96px))",
    pointerEvents: "none",
    zIndex: 55,
  },
  panel: {
    background: "rgba(6, 10, 20, 0.92)",
    border: "1px solid rgba(0, 180, 255, 0.16)",
    borderRadius: 16,
    padding: "12px 16px",
    boxShadow: "0 0 24px rgba(0, 180, 255, 0.08)",
  },
  header: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 12,
  },
  phaseRow: {
    display: "flex",
    alignItems: "center",
    gap: 8,
    minWidth: 0,
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: "50%",
    flexShrink: 0,
  },
  phase: {
    fontSize: 10,
    letterSpacing: 2,
    whiteSpace: "nowrap",
  },
  brand: {
    fontSize: 10,
    letterSpacing: 2,
    color: "rgba(0, 180, 255, 0.42)",
    whiteSpace: "nowrap",
  },
  statusLine: {
    display: "flex",
    alignItems: "baseline",
    gap: 10,
    marginTop: 8,
  },
  statusLabel: {
    color: "rgba(0, 180, 255, 0.4)",
    fontFamily: "var(--font-mono)",
    fontSize: 10,
    letterSpacing: 1.6,
    flexShrink: 0,
  },
  statusText: {
    color: "rgba(220, 245, 255, 0.92)",
    fontSize: 13,
    lineHeight: 1.5,
    minWidth: 0,
  },
  traceBlock: {
    marginTop: 10,
    paddingTop: 10,
    borderTop: "1px solid rgba(0, 180, 255, 0.08)",
  },
  traceLabel: {
    color: "rgba(0, 180, 255, 0.38)",
    fontFamily: "var(--font-mono)",
    fontSize: 10,
    letterSpacing: 1.5,
    marginBottom: 6,
  },
  traceText: {
    color: "rgba(255, 235, 190, 0.92)",
    fontSize: 12,
    lineHeight: 1.55,
    whiteSpace: "pre-wrap",
  },
  responseText: {
    color: "rgba(200, 245, 255, 0.95)",
    fontSize: 13,
    lineHeight: 1.6,
    whiteSpace: "pre-wrap",
  },
  cursor: {
    color: "rgba(0, 180, 255, 0.55)",
    animation: "blink 1s step-end infinite",
    marginLeft: 2,
  },
};
