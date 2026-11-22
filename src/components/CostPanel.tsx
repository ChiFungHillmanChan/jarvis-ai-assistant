import { memo, useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import { useUiLocale } from "../i18n";
import Select from "../ui/Select";
import {
  GROUP_BYS, PERIODS, emptyPeriodLine, expensiveHeading, expensiveRequests, formatUsd, groupByLabel, groupColumnLabel,
  groupRows, historyLine, perDayLine, periodLabel, tokenFigures, unpricedNote, usageHeadline,
  type BlendedCost, type CostStats, type GroupBy, type PeriodDays, type UsageGroup,
} from "../lib/usage";

/**
 * Usage recorded on this Mac over a chosen period, from the per-request rows
 * the backend writes as each reply arrives (ai/cost.rs) and, past the
 * per-request window, the daily totals they are moved into (ai/usage.rs).
 * One row is one chat turn on Gemini, Claude, OpenAI-compatible services and
 * local models, but one request on Bedrock, where a tool turn is several
 * rows; single completions are one row per request everywhere (lib/usage.ts
 * CostStats). "Requests" counts rows. Money is shown only where the price is
 * known.
 *
 * The period is the panel's own unless a parent passes `days` and
 * `onDaysChange` (Settings > Usage, where the panel is "Show details" and
 * shares the page's period).
 */
export default memo(function CostPanel(props: { days?: PeriodDays; onDaysChange?(days: PeriodDays): void }) {
  const { text } = useUiLocale();
  const [ownDays, setOwnDays] = useState<PeriodDays>(30);
  const days = props.days ?? ownDays;
  const setDays = props.onDaysChange ?? setOwnDays;
  const [groupBy, setGroupBy] = useState<GroupBy>("model");
  const { data, error, loading } = useTauriCommand<CostStats>("get_cost_stats", { days });
  const { data: blended, error: blendedError } = useTauriCommand<BlendedCost>("get_blended_cost", { days });
  const { data: groups, error: groupsError } = useTauriCommand<UsageGroup[]>("get_usage_groups", { days, groupBy });

  const controls = (
    <div style={styles.controls}>
      <Select label={text("Period", "期間")} showLabel value={String(days)} onChange={(v) => setDays(Number(v) as PeriodDays)}
        options={PERIODS.map((period) => ({ value: String(period), label: periodLabel(period, text) }))} />
      <Select label={text("Group by", "分組方式")} showLabel value={groupBy} onChange={(v) => setGroupBy(v as GroupBy)}
        options={GROUP_BYS.map((by) => ({ value: by, label: groupByLabel(by, text) }))} />
    </div>
  );
  const history = data && <div style={styles.note}>{historyLine(data.per_request_days, text)}</div>;

  if (loading) {
    return <div style={styles.container}>{controls}<div style={styles.muted}>{text("Loading…", "載入中…")}</div></div>;
  }
  if (error) {
    return <div style={styles.container}>{controls}<div role="alert" style={styles.error}>{error}</div></div>;
  }
  if (!data || data.total_requests === 0) {
    return <div style={styles.container}>{controls}<div style={styles.muted}>{emptyPeriodLine(days, text)}</div>{history}</div>;
  }

  const headline = usageHeadline(data, text);
  const perDay = blended ? perDayLine(blended, days, text) : null;
  const unpriced = unpricedNote(data, text);
  const expensive = expensiveRequests(data);
  const rows = groups ? groupRows(groups, groupBy, text) : [];
  return (
    <div style={styles.container}>
      {controls}
      <div style={styles.kpiRow}>
        <Kpi label={text("Total", "總額")} value={headline.total} />
        <Kpi label={headline.perRequestLabel} value={headline.perRequest} />
        <Kpi label={text("Requests", "請求次數")} value={String(data.total_requests)} />
        <Kpi label={text("Local model share", "本機模型佔比")} value={`${data.local_share_pct.toFixed(0)}%`} />
      </div>

      {perDay && <div style={styles.note}>{perDay}</div>}
      {!blended && blendedError && (
        <div style={styles.note}>{text(`Cost per day of use unavailable (${blendedError})`, `未能計算每個使用日的費用（${blendedError}）`)}</div>
      )}
      {unpriced && <div style={styles.note}>{unpriced}</div>}

      {groupsError && <div role="alert" style={{ ...styles.error, marginTop: 16 }}>{groupsError}</div>}
      {!groups && !groupsError && <div style={{ ...styles.muted, marginTop: 16 }}>{text("Loading…", "載入中…")}</div>}
      {groups && (
        <div style={styles.tableWrap}>
          <table style={styles.table}>
            <thead>
              <tr>
                <th scope="col" style={styles.thName}>{groupColumnLabel(groupBy, days, text)}</th>
                <th scope="col" style={styles.th}>{text("Requests", "請求次數")}</th>
                <th scope="col" style={styles.th}>{text("Input", "輸入")}</th>
                <th scope="col" style={styles.th}>{text("Output", "輸出")}</th>
                <th scope="col" style={styles.th}>{text("Cache read", "快取讀取")}</th>
                <th scope="col" style={styles.th}>{text("Cache write", "快取寫入")}</th>
                <th scope="col" style={styles.th}>{text("Cost", "費用")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.id}>
                  <th scope="row" style={styles.tdName}>
                    <span style={styles.name} title={row.name}>{row.name}</span>
                    {row.detail && <span style={styles.detail}>{row.detail}</span>}
                  </th>
                  <td style={styles.td}>{row.requests}</td>
                  <td style={styles.td}>
                    {row.input}
                    {row.characters && <span style={styles.detail}>{row.characters}</span>}
                  </td>
                  <td style={styles.td}>{row.output}</td>
                  <td style={styles.td}>{row.cacheRead}</td>
                  <td style={styles.td}>{row.cacheWrite}</td>
                  <td style={styles.td}>{row.cost}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {expensive.length > 0 && (
        <>
          <div style={styles.subheader}>{expensiveHeading(data.expensive_days, text)}</div>
          <div style={styles.list}>
            {expensive.map((t, i) => (
              <div key={i} style={styles.row}>
                {/* The whole id: for Bedrock, the prefix says which profile was sent. */}
                <span style={styles.model} title={t.model_id}>{t.model_id}</span>
                <span style={styles.muted}>{tokenFigures(t, text)}</span>
                <span style={styles.amount}>{formatUsd(t.usd_cost ?? 0)}</span>
              </div>
            ))}
          </div>
        </>
      )}
      {history}
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
  controls: { display: "flex", flexWrap: "wrap", gap: 16, marginBottom: 14 },
  subheader: { fontSize: 12, color: "var(--text-secondary)", marginTop: 16, marginBottom: 6 },
  note: { fontSize: 12, color: "var(--text-secondary)", marginTop: 10, lineHeight: 1.5 },
  kpiRow: { display: "flex", flexWrap: "wrap", gap: 20 },
  kpi: { display: "flex", flexDirection: "column", gap: 2 },
  kpiLabel: { fontSize: 11, color: "var(--text-secondary)" },
  kpiValue: { fontSize: 16, fontFamily: "var(--font-mono)", color: "var(--text-primary)" },
  tableWrap: { marginTop: 16, overflowX: "auto" },
  table: { width: "100%", borderCollapse: "collapse", fontFamily: "var(--font-mono)", fontSize: 12 },
  thName: { textAlign: "left", fontWeight: "normal", color: "var(--text-secondary)", padding: "4px 12px 6px 0", borderBottom: "1px solid var(--border-primary)" },
  th: { textAlign: "right", fontWeight: "normal", color: "var(--text-secondary)", padding: "4px 0 6px 12px", borderBottom: "1px solid var(--border-primary)", whiteSpace: "nowrap" },
  tdName: { textAlign: "left", fontWeight: "normal", padding: "5px 12px 5px 0", verticalAlign: "top", maxWidth: 260 },
  td: { textAlign: "right", padding: "5px 0 5px 12px", verticalAlign: "top", whiteSpace: "nowrap", color: "var(--text-primary)" },
  name: { display: "block", color: "var(--text-primary)", overflowWrap: "anywhere" },
  detail: { display: "block", color: "var(--text-secondary)", fontSize: 11 },
  list: { display: "flex", flexDirection: "column", gap: 4 },
  row: { display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto auto", gap: 12, alignItems: "center", fontFamily: "var(--font-mono)", fontSize: 12 },
  model: { color: "var(--text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
  muted: { color: "var(--text-secondary)", fontSize: 12 },
  amount: { color: "var(--text-primary)", textAlign: "right" },
  error: { color: "var(--accent-urgent)", fontSize: 12 },
};
