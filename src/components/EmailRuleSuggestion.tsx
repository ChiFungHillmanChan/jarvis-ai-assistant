import { useEffect, useState } from "react";
import type { EmailRule } from "../lib/types";
import { getSuggestedRules, acceptEmailRule, dismissEmailRule } from "../lib/commands";

export default function EmailRuleSuggestion() {
  const [rules, setRules] = useState<EmailRule[]>([]);

  useEffect(() => { getSuggestedRules().then(setRules); }, []);

  if (rules.length === 0) return null;

  async function handleAccept(id: number) {
    await acceptEmailRule(id);
    setRules((prev) => prev.filter((r) => r.id !== id));
  }

  async function handleDismiss(id: number) {
    await dismissEmailRule(id);
    setRules((prev) => prev.filter((r) => r.id !== id));
  }

  return (
    <div style={styles.container}>
      {rules.map((rule) => (
        <div key={rule.id} style={styles.banner}>
          <div style={styles.text}>
            <span style={styles.label}>AUTO-ARCHIVE SUGGESTION</span>
            <span style={styles.sender}>
              Emails from <strong>{rule.sender}</strong> archived {rule.archive_count} times
            </span>
          </div>
          <div style={styles.actions}>
            <button onClick={() => handleAccept(rule.id)} style={styles.acceptBtn}>ENABLE</button>
            <button onClick={() => handleDismiss(rule.id)} style={styles.dismissBtn}>DISMISS</button>
          </div>
        </div>
      ))}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", flexDirection: "column", gap: 6, marginBottom: 12 },
  banner: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "8px 12px", border: "1px solid rgba(255, 180, 0, 0.2)", borderRadius: 8, background: "rgba(255, 180, 0, 0.04)" },
  text: { display: "flex", flexDirection: "column", gap: 2 },
  label: { color: "rgba(255, 180, 0, 0.7)", fontSize: 8, fontFamily: "var(--font-mono)", letterSpacing: 1.5 },
  sender: { color: "rgba(0, 180, 255, 0.7)", fontSize: 11 },
  actions: { display: "flex", gap: 6 },
  acceptBtn: { background: "rgba(16, 185, 129, 0.1)", border: "1px solid rgba(16, 185, 129, 0.3)", borderRadius: 4, padding: "4px 10px", color: "rgba(16, 185, 129, 0.8)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  dismissBtn: { background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "4px 10px", color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
};
