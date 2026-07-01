import { memo } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";

interface ProviderBreakdown {
  provider: string;
  turns: number;
  usd_cost: number;
  input_tokens: number;
  output_tokens: number;
}

interface ExpensiveTurn {
  ts: number;
  provider: string;
  model_id: string;
  workload: string;
  input_tokens: number;
  output_tokens: number;
  usd_cost: number;
  latency_ms: number;
}

interface CostStats {
  total_turns_30d: number;
  total_usd_30d: number;
  avg_usd_per_turn_30d: number;
  local_turns_30d: number;
  local_share_pct: number;
  by_provider_30d: ProviderBreakdown[];
  top_expensive_30d: ExpensiveTurn[];
}

function formatUsd(usd: number): string {
  if (usd === 0) return "$0";
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  if (usd < 1) return `$${usd.toFixed(3)}`;
  return `$${usd.toFixed(2)}`;
}

function shortModel(id: string): string {
  if (id.length <= 28) return id;
  const parts = id.split(".");
  return parts[parts.length - 1] || id.slice(-28);
}

export default memo(function CostPanel() {
  const { data, error, loading } = useTauriCommand<CostStats>("get_cost_stats");

  if (loading) {
    return (
      <div style={styles.container}>
        <div style={styles.header}>INFERENCE COST · 30D</div>
        <div style={styles.loading}>loading...</div>
      </div>
    );
  }
  if (error) {
    return (
      <div style={styles.container}>
        <div style={styles.header}>INFERENCE COST · 30D</div>
        <div style={styles.error}>{error}</div>
      </div>
    );
  }
  if (!data || data.total_turns_30d === 0) {
    return (
      <div style={styles.container}>
        <div style={styles.header}>INFERENCE COST · 30D</div>
        <div style={styles.empty}>
          No inference recorded yet. Send a chat to start tracking.
        </div>
      </div>
    );
  }

  return (
    <div style={styles.container}>
      <div style={styles.header}>INFERENCE COST · 30D</div>
      <div style={styles.kpiRow}>
        <Kpi label="Total" value={formatUsd(data.total_usd_30d)} />
        <Kpi label="Per turn" value={formatUsd(data.avg_usd_per_turn_30d)} />
        <Kpi label="Turns" value={String(data.total_turns_30d)} />
        <Kpi
          label="Local share"
          value={`${data.local_share_pct.toFixed(0)}%`}
          highlight={data.local_share_pct >= 50}
        />
      </div>

      <div style={styles.subheader}>BY PROVIDER</div>
      <div style={styles.providerList}>
        {data.by_provider_30d.map((p) => (
          <div key={p.provider} style={styles.providerRow}>
            <span style={styles.providerName}>{p.provider}</span>
            <span style={styles.providerTurns}>{p.turns}</span>
            <span style={styles.providerCost}>{formatUsd(p.usd_cost)}</span>
          </div>
        ))}
      </div>

      {data.top_expensive_30d.length > 0 && (
        <>
          <div style={styles.subheader}>TOP 5 EXPENSIVE TURNS</div>
          <div style={styles.expensiveList}>
            {data.top_expensive_30d.map((t, i) => (
              <div key={i} style={styles.expensiveRow}>
                <span style={styles.expensiveModel}>{shortModel(t.model_id)}</span>
                <span style={styles.expensiveTokens}>
                  {t.input_tokens}+{t.output_tokens}
                </span>
                <span style={styles.expensiveCost}>{formatUsd(t.usd_cost)}</span>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
});

function Kpi({ label, value, highlight }: { label: string; value: string; highlight?: boolean }) {
  return (
    <div style={styles.kpi}>
      <div style={styles.kpiLabel}>{label}</div>
      <div style={{ ...styles.kpiValue, ...(highlight ? { color: "rgba(0, 220, 180, 1)" } : {}) }}>
        {value}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    background: "rgba(0, 30, 50, 0.4)",
    border: "1px solid rgba(0, 180, 255, 0.2)",
    borderRadius: 6,
    padding: 14,
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 12,
    color: "rgba(180, 220, 255, 0.9)",
  },
  header: {
    fontSize: 11,
    letterSpacing: 1.5,
    color: "rgba(0, 180, 255, 0.8)",
    marginBottom: 12,
  },
  subheader: {
    fontSize: 10,
    letterSpacing: 1.2,
    color: "rgba(0, 180, 255, 0.5)",
    marginTop: 14,
    marginBottom: 6,
  },
  kpiRow: { display: "flex", gap: 16 },
  kpi: { display: "flex", flexDirection: "column", gap: 2 },
  kpiLabel: { fontSize: 10, color: "rgba(180, 220, 255, 0.5)", letterSpacing: 1 },
  kpiValue: { fontSize: 16, color: "rgba(220, 240, 255, 1)" },
  providerList: { display: "flex", flexDirection: "column", gap: 4 },
  providerRow: { display: "grid", gridTemplateColumns: "1fr auto auto", gap: 12, alignItems: "center" },
  providerName: { color: "rgba(220, 240, 255, 0.9)" },
  providerTurns: { color: "rgba(180, 220, 255, 0.5)", fontSize: 11 },
  providerCost: { color: "rgba(0, 220, 180, 0.9)", textAlign: "right" },
  expensiveList: { display: "flex", flexDirection: "column", gap: 3 },
  expensiveRow: {
    display: "grid",
    gridTemplateColumns: "1fr auto auto",
    gap: 12,
    fontSize: 11,
    color: "rgba(180, 220, 255, 0.7)",
  },
  expensiveModel: { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
  expensiveTokens: { color: "rgba(180, 220, 255, 0.4)" },
  expensiveCost: { color: "rgba(255, 180, 100, 0.9)", textAlign: "right" },
  loading: { color: "rgba(180, 220, 255, 0.4)", fontSize: 11 },
  error: { color: "rgba(255, 100, 100, 0.8)", fontSize: 11 },
  empty: { color: "rgba(180, 220, 255, 0.4)", fontSize: 11, lineHeight: 1.5 },
};
