import { memo } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import { useUiLocale } from "../i18n";
import { formatUsd, monthlyLine, usageHeadline, type BlendedCost, type CostStats } from "../lib/usage";

function shortModel(id: string): string {
  if (id.length <= 28) return id;
  const parts = id.split(".");
  return parts[parts.length - 1] || id.slice(-28);
}

/**
 * Usage recorded on this Mac over the last 30 days: one row per request,
 * written by the backend as each request is answered (ai/cost.rs). Money is
 * shown only where the price is known; see lib/usage.ts.
 */
export default memo(function CostPanel() {
  const { text } = useUiLocale();
  const { data, error, loading } = useTauriCommand<CostStats>("get_cost_stats");
  const { data: blended, error: blendedError } = useTauriCommand<BlendedCost>("get_blended_cost", { days: 30 });
  const header = <div style={styles.header}>{text("Last 30 days", "最近 30 日")}</div>;

  if (loading) {
    return <div style={styles.container}>{header}<div style={styles.muted}>{text("Loading…", "載入中…")}</div></div>;
  }
  if (error) {
    return <div style={styles.container}>{header}<div role="alert" style={styles.error}>{error}</div></div>;
  }
  if (!data || data.total_turns_30d === 0) {
    return <div style={styles.container}>{header}<div style={styles.muted}>{text("No usage recorded on this Mac yet.", "此 Mac 尚未記錄任何用量。")}</div></div>;
  }

  const headline = usageHeadline(data, text);
  return (
    <div style={styles.container}>
      {header}
      <div style={styles.kpiRow}>
        <Kpi label={text("Total", "總額")} value={headline.total} />
        <Kpi label={headline.perRequestLabel} value={headline.perRequest} />
        <Kpi label={text("Requests", "請求次數")} value={String(data.total_turns_30d)} />
        <Kpi label={text("Local model share", "本機模型佔比")} value={`${data.local_share_pct.toFixed(0)}%`} />
      </div>

      {blended && <div style={styles.note}>{monthlyLine(blended, text)}</div>}
      {!blended && blendedError && (
        <div style={styles.note}>{text(`Monthly rate unavailable (${blendedError})`, `未能計算每月費用（${blendedError}）`)}</div>
      )}
      {data.price_unknown_turns_30d > 0 && (
        <div style={styles.note}>
          {text(
            `${data.price_unknown_turns_30d} requests with price unknown, not in the total`,
            `${data.price_unknown_turns_30d} 次請求價格未知，未計入總額`,
          )}
        </div>
      )}

      <div style={styles.subheader}>{text("By provider", "按供應商")}</div>
      <div style={styles.list}>
        {data.by_provider_30d.map((p) => (
          <div key={p.provider} style={styles.row}>
            <span style={styles.name}>{p.provider}</span>
            <span style={styles.muted}>{p.turns}</span>
            <span style={styles.amount}>
              {p.usd_cost === null ? `${(p.input_tokens + p.output_tokens).toLocaleString()} tokens` : p.price_unknown_turns === p.turns ? text("price unknown", "價格未知") : formatUsd(p.usd_cost)}
              {p.usd_cost !== null && p.price_unknown_turns > 0 && p.price_unknown_turns < p.turns ? text(" + price unknown", " + 價格未知") : ""}
            </span>
          </div>
        ))}
      </div>

      {data.top_expensive_30d.length > 0 && (
        <>
          <div style={styles.subheader}>{text("Most expensive requests", "費用最高的請求")}</div>
          <div style={styles.list}>
            {data.top_expensive_30d.map((t, i) => (
              <div key={i} style={styles.row}>
                <span style={styles.model}>{shortModel(t.model_id)}</span>
                <span style={styles.muted}>{t.input_tokens}+{t.output_tokens}</span>
                <span style={styles.amount}>{t.price_unknown ? text("price unknown", "價格未知") : t.usd_cost === null ? text("free", "免費") : formatUsd(t.usd_cost)}</span>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
});

function Kpi({ label, value }: { label: string; value: string }) {
  return (
    <div style={styles.kpi}>
      <div style={styles.kpiLabel}>{label}</div>
      <div style={styles.kpiValue}>{value}</div>
    </div>
  );
}

// Theme variables, so the panel reads in the light and the dark appearance.
const styles: Record<string, React.CSSProperties> = {
  container: {
    border: "1px solid var(--border-primary)",
    borderRadius: 6,
    padding: 14,
    fontSize: 13,
    color: "var(--text-primary)",
  },
  header: { fontSize: 12, color: "var(--text-secondary)", marginBottom: 12 },
  subheader: { fontSize: 12, color: "var(--text-secondary)", marginTop: 16, marginBottom: 6 },
  note: { fontSize: 12, color: "var(--text-secondary)", marginTop: 10, lineHeight: 1.5 },
  kpiRow: { display: "flex", flexWrap: "wrap", gap: 20 },
  kpi: { display: "flex", flexDirection: "column", gap: 2 },
  kpiLabel: { fontSize: 11, color: "var(--text-secondary)" },
  kpiValue: { fontSize: 16, fontFamily: "var(--font-mono)", color: "var(--text-primary)" },
  list: { display: "flex", flexDirection: "column", gap: 4 },
  row: { display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto auto", gap: 12, alignItems: "center", fontFamily: "var(--font-mono)", fontSize: 12 },
  name: { color: "var(--text-primary)", overflowWrap: "anywhere" },
  model: { color: "var(--text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
  muted: { color: "var(--text-secondary)", fontSize: 12 },
  amount: { color: "var(--text-primary)", textAlign: "right" },
  error: { color: "var(--accent-urgent)", fontSize: 12 },
};
