// jarvis/src/components/Briefing.tsx
import { useState, useEffect } from "react";
import type { BriefingResult } from "../lib/types";
import { getBriefing, speakBriefing } from "../lib/commands";

export default function Briefing() {
  const [briefing, setBriefing] = useState<BriefingResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [dismissed, setDismissed] = useState(false);
  const [speaking, setSpeaking] = useState(false);

  useEffect(() => {
    getBriefing()
      .then(setBriefing)
      .catch((e) => console.error("Briefing failed:", e))
      .finally(() => setLoading(false));
  }, []);

  if (dismissed || (!loading && !briefing)) return null;

  async function handleSpeak() {
    setSpeaking(true);
    try { await speakBriefing(); }
    catch (e) { console.error(e); }
    finally { setSpeaking(false); }
  }

  return (
    <div style={styles.container} className="animate-fade-in">
      <div style={styles.header}>
        <span className="system-text">DAILY BRIEFING</span>
        <div style={styles.actions}>
          <button onClick={handleSpeak} disabled={speaking} style={styles.speakBtn}>
            {speaking ? "SPEAKING..." : "SPEAK"}
          </button>
          <button onClick={() => setDismissed(true)} style={styles.dismissBtn}>DISMISS</button>
        </div>
      </div>
      {loading ? (
        <div className="system-text animate-glow" style={{ padding: 12 }}>GENERATING BRIEFING...</div>
      ) : briefing && (
        <div style={styles.body}>
          <div style={styles.greeting}>{briefing.greeting}</div>
          <div style={styles.text}>{briefing.briefing}</div>
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { border: "1px solid rgba(0, 180, 255, 0.2)", borderRadius: 8, background: "rgba(0, 180, 255, 0.03)", marginBottom: 16, overflow: "hidden" },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "8px 12px", borderBottom: "1px solid rgba(0, 180, 255, 0.1)" },
  actions: { display: "flex", gap: 6 },
  speakBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 4, padding: "3px 8px", color: "rgba(0, 180, 255, 0.8)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  dismissBtn: { background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "3px 8px", color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  body: { padding: 12 },
  greeting: { color: "rgba(0, 180, 255, 0.7)", fontSize: 14, fontWeight: 300, marginBottom: 8 },
  text: { color: "rgba(0, 180, 255, 0.6)", fontSize: 12, lineHeight: 1.6, whiteSpace: "pre-wrap" as const },
};
