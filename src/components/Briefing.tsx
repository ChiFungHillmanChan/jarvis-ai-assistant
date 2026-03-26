import { useState, useEffect, useRef, memo } from "react";
import type { BriefingResult } from "../lib/types";
import { getBriefing, speakBriefing } from "../lib/commands";

// Cache briefing at module level so it only generates once per app session
let cachedBriefing: BriefingResult | null = null;
let briefingLoaded = false;
let briefingLoading = false;

export default memo(function Briefing() {
  const [briefing, setBriefing] = useState<BriefingResult | null>(cachedBriefing);
  const [loading, setLoading] = useState(!briefingLoaded);
  const [dismissed, setDismissed] = useState(false);
  const [speaking, setSpeaking] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;

    // Only fetch once per app session
    if (briefingLoaded) {
      setBriefing(cachedBriefing);
      setLoading(false);
      return;
    }
    if (briefingLoading) return;

    briefingLoading = true;
    getBriefing()
      .then((result) => {
        cachedBriefing = result;
        briefingLoaded = true;
        if (mounted.current) {
          setBriefing(result);
          setLoading(false);
        }
      })
      .catch((e) => {
        console.error("Briefing failed:", e);
        briefingLoaded = true;
        if (mounted.current) setLoading(false);
      })
      .finally(() => { briefingLoading = false; });

    return () => { mounted.current = false; };
  }, []);

  // Dismissed state -- show small "reopen" button
  if (dismissed) {
    return (
      <div style={styles.reopenBar}>
        <button onClick={() => setDismissed(false)} style={styles.reopenBtn}>
          SHOW BRIEFING
        </button>
      </div>
    );
  }

  if (!loading && !briefing) return null;

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
})

const styles: Record<string, React.CSSProperties> = {
  container: { border: "1px solid rgba(0, 180, 255, 0.2)", borderRadius: 8, background: "rgba(10, 14, 26, 0.92)", marginBottom: 16, overflow: "hidden" },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "8px 12px", borderBottom: "1px solid rgba(0, 180, 255, 0.1)" },
  actions: { display: "flex", gap: 6 },
  speakBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 4, padding: "3px 8px", color: "rgba(0, 180, 255, 0.8)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  dismissBtn: { background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "3px 8px", color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  body: { padding: 12 },
  greeting: { color: "rgba(0, 180, 255, 0.7)", fontSize: 14, fontWeight: 300, marginBottom: 8 },
  text: { color: "rgba(0, 180, 255, 0.6)", fontSize: 12, lineHeight: 1.6, whiteSpace: "pre-wrap" as const },
  reopenBar: { marginBottom: 8, display: "flex", justifyContent: "center" },
  reopenBtn: { background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.12)", borderRadius: 12, padding: "3px 12px", color: "rgba(0, 180, 255, 0.35)", fontSize: 8, fontFamily: "var(--font-mono)", cursor: "pointer", letterSpacing: 1 },
};
