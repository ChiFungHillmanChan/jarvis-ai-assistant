import { memo } from "react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface Series {
  name: string;
  data: number[];
}

interface LineChartData {
  labels: string[];
  series: Series[];
}

interface PieSegment {
  label: string;
  value: number;
}

interface PieChartData {
  segments: PieSegment[];
}

// Bar chart uses the same format as line chart
type BarChartData = LineChartData;

interface InlineChartProps {
  chartType: string;
  data: Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// Color palette
// ---------------------------------------------------------------------------

const COLORS = [
  "rgba(0,180,255,0.8)",
  "rgba(16,185,129,0.8)",
  "rgba(255,180,0,0.8)",
  "rgba(168,85,247,0.8)",
];

// ---------------------------------------------------------------------------
// Line chart
// ---------------------------------------------------------------------------

function LineChart({ data }: { data: LineChartData }) {
  const { labels, series } = data;
  if (!series || series.length === 0) return null;

  const W = 200;
  const H = 60;
  const padX = 4;
  const padTop = 4;
  const padBot = 14;
  const plotW = W - padX * 2;
  const plotH = H - padTop - padBot;

  // Compute global min/max across all series
  let allMin = Infinity;
  let allMax = -Infinity;
  for (const s of series) {
    for (const v of s.data) {
      if (v < allMin) allMin = v;
      if (v > allMax) allMax = v;
    }
  }
  if (allMin === allMax) {
    allMin -= 1;
    allMax += 1;
  }
  const range = allMax - allMin;

  const toX = (i: number, count: number) =>
    padX + (count > 1 ? (i / (count - 1)) * plotW : plotW / 2);
  const toY = (v: number) =>
    padTop + plotH - ((v - allMin) / range) * plotH;

  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      style={{ width: "100%", maxWidth: 320, height: "auto", display: "block" }}
    >
      <defs>
        {series.map((_, si) => (
          <linearGradient
            key={si}
            id={`line-grad-${si}`}
            x1="0"
            y1="0"
            x2="0"
            y2="1"
          >
            <stop
              offset="0%"
              stopColor={COLORS[si % COLORS.length]}
              stopOpacity={0.25}
            />
            <stop
              offset="100%"
              stopColor={COLORS[si % COLORS.length]}
              stopOpacity={0}
            />
          </linearGradient>
        ))}
      </defs>

      {series.map((s, si) => {
        const color = COLORS[si % COLORS.length];
        const pts = s.data.map(
          (v, i) => `${toX(i, s.data.length)},${toY(v)}`
        );
        const polyline = pts.join(" ");

        // Area fill: close path along bottom
        const firstX = toX(0, s.data.length);
        const lastX = toX(s.data.length - 1, s.data.length);
        const fillPts = `${firstX},${padTop + plotH} ${polyline} ${lastX},${padTop + plotH}`;

        const lastPt = s.data[s.data.length - 1];

        return (
          <g key={si}>
            <polygon
              points={fillPts}
              fill={`url(#line-grad-${si})`}
            />
            <polyline
              points={polyline}
              fill="none"
              stroke={color}
              strokeWidth={1.2}
              strokeLinejoin="round"
            />
            {/* Last data point dot */}
            <circle
              cx={toX(s.data.length - 1, s.data.length)}
              cy={toY(lastPt)}
              r={2}
              fill={color}
            />
          </g>
        );
      })}

      {/* X-axis labels */}
      {labels &&
        labels.map((label, i) => (
          <text
            key={i}
            x={toX(i, labels.length)}
            y={H - 2}
            textAnchor="middle"
            fill="rgba(0,180,255,0.4)"
            fontSize={4}
            fontFamily="var(--font-mono)"
          >
            {label}
          </text>
        ))}
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Pie chart
// ---------------------------------------------------------------------------

function PieChart({ data }: { data: PieChartData }) {
  const { segments } = data;
  if (!segments || segments.length === 0) return null;

  const total = segments.reduce((sum, s) => sum + s.value, 0);
  if (total === 0) return null;

  const cx = 40;
  const cy = 40;
  const r = 28;
  const circumference = 2 * Math.PI * r;

  let offset = 0;

  return (
    <div>
      <svg
        viewBox="0 0 80 80"
        style={{ width: 80, height: 80, display: "block", margin: "0 auto" }}
      >
        {/* Background ring */}
        <circle
          cx={cx}
          cy={cy}
          r={r}
          fill="none"
          stroke="rgba(0,180,255,0.06)"
          strokeWidth={8}
        />

        {segments.map((seg, i) => {
          const pct = seg.value / total;
          const dash = pct * circumference;
          const gap = circumference - dash;
          const color = COLORS[i % COLORS.length];
          const currentOffset = offset;
          offset += dash;

          return (
            <circle
              key={i}
              cx={cx}
              cy={cy}
              r={r}
              fill="none"
              stroke={color}
              strokeWidth={8}
              strokeDasharray={`${dash} ${gap}`}
              strokeDashoffset={-currentOffset}
              strokeLinecap="round"
              transform={`rotate(-90 ${cx} ${cy})`}
            />
          );
        })}

        {/* Center percentage for single segment */}
        {segments.length === 1 && (
          <text
            x={cx}
            y={cy + 1}
            textAnchor="middle"
            dominantBaseline="central"
            fill="rgba(0,180,255,0.8)"
            fontSize={10}
            fontFamily="var(--font-mono)"
          >
            {Math.round((segments[0].value / total) * 100)}%
          </text>
        )}
      </svg>

      {/* Legend */}
      <div style={pieStyles.legend}>
        {segments.map((seg, i) => (
          <div key={i} style={pieStyles.legendItem}>
            <span
              style={{
                ...pieStyles.legendDot,
                background: COLORS[i % COLORS.length],
              }}
            />
            <span style={pieStyles.legendLabel}>{seg.label}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Bar chart (div-based)
// ---------------------------------------------------------------------------

function BarChart({ data }: { data: BarChartData }) {
  const { labels, series } = data;
  if (!series || series.length === 0) return null;

  // Find max value for scaling
  let maxVal = 0;
  for (const s of series) {
    for (const v of s.data) {
      if (v > maxVal) maxVal = v;
    }
  }
  if (maxVal === 0) maxVal = 1;

  const groupCount = labels ? labels.length : (series[0]?.data.length ?? 0);
  const barHeight = 36;

  return (
    <div style={barStyles.container}>
      {Array.from({ length: groupCount }, (_, gi) => (
        <div key={gi} style={barStyles.group}>
          <div style={barStyles.bars}>
            {series.map((s, si) => {
              const val = s.data[gi] ?? 0;
              const pct = (val / maxVal) * 100;
              const color = COLORS[si % COLORS.length];
              return (
                <div
                  key={si}
                  style={{
                    height: 6,
                    width: `${pct}%`,
                    minWidth: val > 0 ? 4 : 0,
                    maxWidth: "100%",
                    background: color,
                    borderRadius: 2,
                  }}
                  title={`${s.name}: ${val}`}
                />
              );
            })}
          </div>
          {labels && labels[gi] && (
            <div style={barStyles.label}>{labels[gi]}</div>
          )}
        </div>
      ))}

      {/* Bar height reference -- keeps container from collapsing */}
      <div style={{ height: 0, width: 0, overflow: "hidden" }}>
        {barHeight}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

function InlineChartInner({ chartType, data }: InlineChartProps) {
  const type = chartType.toLowerCase();

  return (
    <div style={wrapperStyle}>
      {type === "line" && <LineChart data={data as unknown as LineChartData} />}
      {type === "pie" && <PieChart data={data as unknown as PieChartData} />}
      {type === "bar" && <BarChart data={data as unknown as BarChartData} />}
      {type !== "line" && type !== "pie" && type !== "bar" && (
        <div style={unknownStyle}>[Chart: {chartType}]</div>
      )}
    </div>
  );
}

const InlineChart = memo(InlineChartInner);
export default InlineChart;

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const wrapperStyle: React.CSSProperties = {
  background: "rgba(0,180,255,0.04)",
  border: "1px solid rgba(0,180,255,0.1)",
  borderRadius: 6,
  padding: 10,
  marginTop: 6,
};

const unknownStyle: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  color: "rgba(0,180,255,0.5)",
};

const pieStyles: Record<string, React.CSSProperties> = {
  legend: {
    display: "flex",
    flexWrap: "wrap",
    gap: 8,
    justifyContent: "center",
    marginTop: 6,
  },
  legendItem: {
    display: "flex",
    alignItems: "center",
    gap: 4,
  },
  legendDot: {
    display: "inline-block",
    width: 6,
    height: 6,
    borderRadius: "50%",
  },
  legendLabel: {
    fontSize: 10,
    fontFamily: "var(--font-mono)",
    color: "rgba(0,180,255,0.6)",
  },
};

const barStyles: Record<string, React.CSSProperties> = {
  container: {
    display: "flex",
    flexDirection: "column",
    gap: 6,
  },
  group: {
    display: "flex",
    flexDirection: "column",
    gap: 2,
  },
  bars: {
    display: "flex",
    flexDirection: "column",
    gap: 2,
  },
  label: {
    fontSize: 10,
    fontFamily: "var(--font-mono)",
    color: "rgba(0,180,255,0.4)",
  },
};
