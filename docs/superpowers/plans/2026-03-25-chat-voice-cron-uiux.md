# Chat, Voice & Cron UIUX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade three UIUX areas -- AI Chat (inline charts, status cards, markdown), Voice (3D sphere integration), and Cron Scheduling (animated conversion flow + timeline) -- to match the holographic design vision.

**Architecture:** Frontend-first approach. Each area (A, B, C) is independent and ships incrementally. Chat adds a message rendering pipeline with markdown/SVG charts/status cards. Voice expands the existing Canvas2D animation loop with mic amplitude, color transitions, and particle effects. Cron adds animated conversion flow and timeline view with a backend migration for human-readable descriptions.

**Tech Stack:** React 18 + TypeScript, inline SVG (no chart library), Canvas2D, Tauri v2 IPC, Rust backend, SQLite with refinery migrations.

**Spec:** `docs/superpowers/specs/2026-03-25-chat-voice-cron-uiux-design.md`

---

## File Structure

### New files
| File | Responsibility |
|------|---------------|
| `src/components/chat/MessageRenderer.tsx` | Parse message content into markdown, charts, status cards |
| `src/components/chat/InlineChart.tsx` | SVG line/pie/bar chart components |
| `src/components/chat/StatusCard.tsx` | Action result card with icon + status badge |
| `src/components/cron/ConversionFlow.tsx` | Animated natural language → cron parsing flow |
| `src/components/cron/CronTimeline.tsx` | Upcoming + recent runs timeline |
| `src/components/cron/CronJobCard.tsx` | Redesigned job card with human-readable schedule |
| `src-tauri/migrations/V6__cron_description.sql` | Add description column to cron_jobs |

### Modified files
| File | Changes |
|------|---------|
| `src/components/ChatMessage.tsx` | Use MessageRenderer instead of plain text |
| `src/components/ChatPanel.tsx` | New input bar, width 440px, remove fullscreen overlay mode |
| `src/components/Sidebar.tsx` | Add CHAT nav item |
| `src/App.tsx` | Add chat full view, remove voice auto-open, pass micAmplitudeRef |
| `src/components/VoiceIndicator.tsx` | Shrink to minimal dot + label |
| `src/components/3d/JarvisScene.tsx` | Mic waveform, color transitions, particle attraction, core glow |
| `src/pages/CronDashboard.tsx` | New vertical layout with conversion flow + grid + expandable detail |
| `src/lib/types.ts` | CronJobView adds description, upcoming_runs |
| `src/lib/commands.ts` | Add getUpcomingRuns wrapper |
| `src-tauri/src/commands/cron.rs` | AI prompt adds description, new get_upcoming_runs command, update CronJobView struct |
| `src-tauri/src/lib.rs` | Register get_upcoming_runs in invoke_handler |
| `src-tauri/src/voice/mod.rs` | Add shared AtomicU32 for mic amplitude + polling emitter |
| `src-tauri/src/ai/tools.rs` | Add render_chart and render_status tool definitions |
| `src-tauri/Cargo.toml` | Add cron = "0.12" as explicit dependency |

---

## A. AI Chat with Inline Tool Results

### Task 1: MessageRenderer -- Markdown Parsing

**Files:**
- Create: `src/components/chat/MessageRenderer.tsx`

- [ ] **Step 1: Create MessageRenderer with markdown parsing**

```tsx
// src/components/chat/MessageRenderer.tsx
import { memo, useMemo } from "react";

interface MessageRendererProps {
  content: string;
}

interface ContentBlock {
  type: "text" | "chart" | "status";
  content: string;
  data?: Record<string, unknown>;
}

function parseBlocks(content: string): ContentBlock[] {
  const blocks: ContentBlock[] = [];
  const tagRegex = /\[(CHART|STATUS):([^\]]+)\]/g;
  let lastIndex = 0;
  let match;

  while ((match = tagRegex.exec(content)) !== null) {
    if (match.index > lastIndex) {
      blocks.push({ type: "text", content: content.slice(lastIndex, match.index) });
    }
    const tagType = match[1].toLowerCase() as "chart" | "status";
    const tagContent = match[2];
    const pipeIndex = tagContent.indexOf("|");
    if (pipeIndex > -1) {
      try {
        const data = JSON.parse(tagContent.slice(pipeIndex + 1));
        blocks.push({ type: tagType, content: tagContent.slice(0, pipeIndex), data });
      } catch {
        blocks.push({ type: "text", content: match[0] });
      }
    } else {
      blocks.push({ type: "text", content: match[0] });
    }
    lastIndex = match.index + match[0].length;
  }

  if (lastIndex < content.length) {
    blocks.push({ type: "text", content: content.slice(lastIndex) });
  }

  return blocks;
}

function renderMarkdown(text: string): JSX.Element[] {
  const lines = text.split("\n");
  const elements: JSX.Element[] = [];
  let listItems: string[] = [];
  let listType: "ul" | "ol" | null = null;

  const flushList = () => {
    if (listItems.length > 0 && listType) {
      const Tag = listType;
      elements.push(
        <Tag key={`list-${elements.length}`} style={mdStyles.list}>
          {listItems.map((item, i) => (
            <li key={i} style={mdStyles.listItem}>{renderInline(item)}</li>
          ))}
        </Tag>
      );
      listItems = [];
      listType = null;
    }
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Headings
    const headingMatch = line.match(/^(#{1,3})\s+(.+)/);
    if (headingMatch) {
      flushList();
      const level = headingMatch[1].length;
      const sizes = [16, 14, 13];
      elements.push(
        <div key={`h-${i}`} style={{ ...mdStyles.heading, fontSize: sizes[level - 1] }}>{renderInline(headingMatch[2])}</div>
      );
      continue;
    }

    // Unordered list
    const ulMatch = line.match(/^[-*]\s+(.+)/);
    if (ulMatch) {
      if (listType === "ol") flushList();
      listType = "ul";
      listItems.push(ulMatch[1]);
      continue;
    }

    // Ordered list
    const olMatch = line.match(/^\d+\.\s+(.+)/);
    if (olMatch) {
      if (listType === "ul") flushList();
      listType = "ol";
      listItems.push(olMatch[1]);
      continue;
    }

    flushList();

    // Empty line
    if (line.trim() === "") {
      continue;
    }

    // Regular paragraph
    elements.push(
      <p key={`p-${i}`} style={mdStyles.paragraph}>{renderInline(line)}</p>
    );
  }

  flushList();
  return elements;
}

function renderInline(text: string): (string | JSX.Element)[] {
  const parts: (string | JSX.Element)[] = [];
  const inlineRegex = /(\*\*(.+?)\*\*)|(\*(.+?)\*)|(\[(.+?)\]\((.+?)\))|(`(.+?)`)/g;
  let lastIdx = 0;
  let m;

  while ((m = inlineRegex.exec(text)) !== null) {
    if (m.index > lastIdx) {
      parts.push(text.slice(lastIdx, m.index));
    }
    if (m[1]) {
      parts.push(<strong key={m.index} style={mdStyles.bold}>{m[2]}</strong>);
    } else if (m[3]) {
      parts.push(<em key={m.index} style={mdStyles.italic}>{m[4]}</em>);
    } else if (m[5]) {
      parts.push(<a key={m.index} href={m[7]} style={mdStyles.link} target="_blank" rel="noopener noreferrer">{m[6]}</a>);
    } else if (m[8]) {
      parts.push(<code key={m.index} style={mdStyles.code}>{m[9]}</code>);
    }
    lastIdx = m.index + m[0].length;
  }

  if (lastIdx < text.length) {
    parts.push(text.slice(lastIdx));
  }

  return parts;
}

export { parseBlocks, renderMarkdown, type ContentBlock };

export default memo(function MessageRenderer({ content }: MessageRendererProps) {
  const blocks = useMemo(() => parseBlocks(content), [content]);

  return (
    <div>
      {blocks.map((block, i) => {
        if (block.type === "text") {
          return <div key={i}>{renderMarkdown(block.content)}</div>;
        }
        if (block.type === "chart" && block.data) {
          // Will be added in Task 2
          return <div key={i} style={mdStyles.placeholder}>[Chart: {block.content}]</div>;
        }
        if (block.type === "status" && block.data) {
          // Will be added in Task 3
          return <div key={i} style={mdStyles.placeholder}>[Status: {block.content}]</div>;
        }
        return null;
      })}
    </div>
  );
});

const mdStyles: Record<string, React.CSSProperties> = {
  heading: { color: "rgba(0, 180, 255, 0.95)", fontWeight: 600, margin: "8px 0 4px 0" },
  paragraph: { margin: "4px 0", lineHeight: 1.6 },
  bold: { color: "rgba(0, 180, 255, 0.95)", fontWeight: 600 },
  italic: { fontStyle: "italic", color: "rgba(0, 180, 255, 0.75)" },
  link: { color: "rgba(0, 180, 255, 0.9)", textDecoration: "underline" },
  code: { background: "rgba(0, 180, 255, 0.08)", padding: "2px 6px", borderRadius: 3, fontFamily: "var(--font-mono)", fontSize: 12 },
  list: { margin: "4px 0", paddingLeft: 18, lineHeight: 1.8 },
  listItem: { color: "rgba(0, 180, 255, 0.8)" },
  placeholder: { color: "rgba(0, 180, 255, 0.4)", fontSize: 11, fontFamily: "var(--font-mono)", padding: "8px 0" },
};
```

- [ ] **Step 2: Verify file compiles**

Run: `npx tsc --noEmit 2>&1 | tail -10`

- [ ] **Step 3: Commit**

```bash
git add src/components/chat/MessageRenderer.tsx
git commit -m "feat(chat): add MessageRenderer with markdown parsing"
```

---

### Task 2: InlineChart -- SVG Charts

**Files:**
- Create: `src/components/chat/InlineChart.tsx`

- [ ] **Step 1: Create InlineChart component with line, pie, and bar charts**

```tsx
// src/components/chat/InlineChart.tsx
import { memo } from "react";

interface LineSeries { name: string; data: number[]; }
interface LineChartData { labels: string[]; series: LineSeries[]; }
interface PieSegment { label: string; value: number; }
interface PieChartData { segments: PieSegment[]; }
interface BarChartData { labels: string[]; series: LineSeries[]; }

type ChartData = LineChartData | PieChartData | BarChartData;

// Color palette from holographic theme
const COLORS = [
  "rgba(0, 180, 255, 0.8)",
  "rgba(16, 185, 129, 0.8)",
  "rgba(255, 180, 0, 0.8)",
  "rgba(168, 85, 247, 0.8)",
];

function LineChart({ data }: { data: LineChartData }) {
  const w = 200, h = 60, padX = 4, padY = 4;
  const allValues = data.series.flatMap(s => s.data);
  const min = Math.min(...allValues);
  const max = Math.max(...allValues);
  const range = max - min || 1;

  return (
    <div style={chartStyles.container}>
      <svg width="100%" height={h} viewBox={`0 0 ${w} ${h}`} style={{ display: "block" }}>
        {data.series.map((series, si) => {
          const points = series.data.map((v, i) => {
            const x = padX + (i / Math.max(series.data.length - 1, 1)) * (w - padX * 2);
            const y = padY + (1 - (v - min) / range) * (h - padY * 2);
            return `${x},${y}`;
          }).join(" ");
          const color = COLORS[si % COLORS.length];

          // Fill gradient
          const firstPoint = `${padX},${h - padY}`;
          const lastPoint = `${padX + ((series.data.length - 1) / Math.max(series.data.length - 1, 1)) * (w - padX * 2)},${h - padY}`;
          const fillPath = `M${firstPoint} L${points} L${lastPoint} Z`;

          return (
            <g key={si}>
              <defs>
                <linearGradient id={`lg-${si}`} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={color.replace("0.8", "0.25")} />
                  <stop offset="100%" stopColor={color.replace("0.8", "0")} />
                </linearGradient>
              </defs>
              <path d={fillPath} fill={`url(#lg-${si})`} />
              <polyline points={points} fill="none" stroke={color} strokeWidth="2" />
              {/* Last point dot */}
              {series.data.length > 0 && (() => {
                const lastIdx = series.data.length - 1;
                const cx = padX + (lastIdx / Math.max(lastIdx, 1)) * (w - padX * 2);
                const cy = padY + (1 - (series.data[lastIdx] - min) / range) * (h - padY * 2);
                return <circle cx={cx} cy={cy} r="3" fill={color} />;
              })()}
            </g>
          );
        })}
      </svg>
      {data.labels && data.labels.length > 0 && (
        <div style={chartStyles.labels}>
          {data.labels.map((l, i) => <span key={i}>{l}</span>)}
        </div>
      )}
    </div>
  );
}

function PieChart({ data }: { data: PieChartData }) {
  const total = data.segments.reduce((sum, s) => sum + s.value, 0);
  if (total === 0) return null;
  const cx = 35, cy = 35, r = 28;
  let cumAngle = -90;

  return (
    <div style={{ ...chartStyles.container, textAlign: "center", maxWidth: 120 }}>
      <svg width="70" height="70" viewBox="0 0 70 70" style={{ display: "block", margin: "0 auto" }}>
        <circle cx={cx} cy={cy} r={r} fill="none" stroke="rgba(0, 180, 255, 0.1)" strokeWidth="8" />
        {data.segments.map((seg, i) => {
          const pct = seg.value / total;
          const dashLen = 2 * Math.PI * r * pct;
          const dashGap = 2 * Math.PI * r * (1 - pct);
          const rotation = cumAngle;
          cumAngle += pct * 360;
          return (
            <circle
              key={i}
              cx={cx} cy={cy} r={r}
              fill="none"
              stroke={COLORS[i % COLORS.length]}
              strokeWidth="8"
              strokeDasharray={`${dashLen} ${dashGap}`}
              strokeLinecap="round"
              transform={`rotate(${rotation} ${cx} ${cy})`}
            />
          );
        })}
        {data.segments.length === 1 && (
          <text x={cx} y={cy + 4} textAnchor="middle" fill="rgba(0, 180, 255, 0.9)" fontSize="13" fontFamily="var(--font-mono)">
            {Math.round((data.segments[0].value / total) * 100)}%
          </text>
        )}
      </svg>
      <div style={{ display: "flex", flexDirection: "column", gap: 2, marginTop: 4 }}>
        {data.segments.map((seg, i) => (
          <div key={i} style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 9, color: "rgba(0, 180, 255, 0.5)" }}>
            <div style={{ width: 6, height: 6, borderRadius: "50%", background: COLORS[i % COLORS.length], flexShrink: 0 }} />
            <span>{seg.label} ({Math.round((seg.value / total) * 100)}%)</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function BarChart({ data }: { data: BarChartData }) {
  const allValues = data.series.flatMap(s => s.data);
  const max = Math.max(...allValues, 1);

  return (
    <div style={chartStyles.container}>
      <div style={{ display: "flex", alignItems: "flex-end", gap: 4, height: 50 }}>
        {data.labels.map((label, i) => (
          <div key={i} style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", gap: 2 }}>
            <div style={{ display: "flex", gap: 1, alignItems: "flex-end", height: 40 }}>
              {data.series.map((series, si) => {
                const h = (series.data[i] / max) * 40;
                return (
                  <div
                    key={si}
                    style={{
                      width: Math.max(8, 24 / data.series.length),
                      height: Math.max(2, h),
                      background: COLORS[si % COLORS.length],
                      borderRadius: "2px 2px 0 0",
                    }}
                  />
                );
              })}
            </div>
            <span style={{ fontSize: 8, color: "rgba(0, 180, 255, 0.3)", fontFamily: "var(--font-mono)" }}>{label}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

interface InlineChartProps {
  chartType: string;
  data: ChartData;
}

export default memo(function InlineChart({ chartType, data }: InlineChartProps) {
  const chartData = data as Record<string, unknown>;

  switch (chartType) {
    case "line":
      return <LineChart data={chartData as unknown as LineChartData} />;
    case "pie":
      return <PieChart data={chartData as unknown as PieChartData} />;
    case "bar":
      return <BarChart data={chartData as unknown as BarChartData} />;
    default:
      return <div style={chartStyles.container}>Unknown chart type: {chartType}</div>;
  }
});

const chartStyles: Record<string, React.CSSProperties> = {
  container: {
    background: "rgba(0, 180, 255, 0.04)",
    border: "1px solid rgba(0, 180, 255, 0.1)",
    borderRadius: 6,
    padding: 10,
    marginTop: 6,
  },
  labels: {
    display: "flex",
    justifyContent: "space-between",
    fontSize: 8,
    color: "rgba(0, 180, 255, 0.3)",
    fontFamily: "var(--font-mono)",
    marginTop: 4,
  },
};
```

- [ ] **Step 2: Wire InlineChart into MessageRenderer**

In `src/components/chat/MessageRenderer.tsx`, replace the chart placeholder:

```tsx
// Add import at top
import InlineChart from "./InlineChart";

// Replace the chart placeholder block in the render:
if (block.type === "chart" && block.data) {
  return <InlineChart key={i} chartType={block.content} data={block.data as any} />;
}
```

- [ ] **Step 3: Commit**

```bash
git add src/components/chat/InlineChart.tsx src/components/chat/MessageRenderer.tsx
git commit -m "feat(chat): add InlineChart SVG components (line, pie, bar)"
```

---

### Task 3: StatusCard -- Action Result Cards

**Files:**
- Create: `src/components/chat/StatusCard.tsx`

- [ ] **Step 1: Create StatusCard component**

```tsx
// src/components/chat/StatusCard.tsx
import { memo } from "react";

interface StatusCardProps {
  statusType: string;
  data: Record<string, unknown>;
}

const STATUS_CONFIG: Record<string, { color: string; label: string; icon: string }> = {
  task_created: { color: "rgba(16, 185, 129", label: "CREATED", icon: "check" },
  task_completed: { color: "rgba(16, 185, 129", label: "COMPLETED", icon: "check" },
  email_synced: { color: "rgba(0, 180, 255", label: "SYNCED", icon: "sync" },
  calendar_synced: { color: "rgba(0, 180, 255", label: "SYNCED", icon: "sync" },
  cron_created: { color: "rgba(16, 185, 129", label: "ACTIVE", icon: "check" },
  action_completed: { color: "rgba(16, 185, 129", label: "DONE", icon: "check" },
};

function StatusIcon({ type, color }: { type: string; color: string }) {
  if (type === "sync") {
    return (
      <svg width="14" height="14" viewBox="0 0 14 14">
        <path d="M7 1v3l2-2M7 1C3.7 1 1 3.7 1 7" fill="none" stroke={`${color}, 0.8)`} strokeWidth="1.5" strokeLinecap="round" />
        <path d="M7 13v-3l-2 2M7 13c3.3 0 6-2.7 6-6" fill="none" stroke={`${color}, 0.8)`} strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    );
  }
  return (
    <svg width="14" height="14" viewBox="0 0 14 14">
      <polyline points="3,7 6,10 11,4" fill="none" stroke={`${color}, 0.8)`} strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

export default memo(function StatusCard({ statusType, data }: StatusCardProps) {
  const config = STATUS_CONFIG[statusType] || { color: "rgba(0, 180, 255", label: "OK", icon: "check" };
  const title = (data.name || data.action || statusType.replace("_", " ")) as string;
  const details: string[] = [];

  if (data.priority) details.push(`Priority: ${String(data.priority).toUpperCase()}`);
  if (data.due) details.push(`Due: ${data.due}`);
  if (data.count) details.push(`${data.count} items`);
  if (data.folder) details.push(`Folder: ${data.folder}`);
  if (data.schedule) details.push(`Schedule: ${data.schedule}`);
  if (data.description) details.push(String(data.description));
  if (data.result) details.push(String(data.result));

  return (
    <div style={{
      background: `${config.color}, 0.06)`,
      border: `1px solid ${config.color}, 0.2)`,
      borderRadius: 6,
      padding: "10px 12px",
      display: "flex",
      alignItems: "center",
      gap: 10,
      marginTop: 6,
    }}>
      <div style={{
        width: 28, height: 28, borderRadius: 6,
        background: `${config.color}, 0.12)`,
        border: `1px solid ${config.color}, 0.25)`,
        display: "flex", alignItems: "center", justifyContent: "center", flexShrink: 0,
      }}>
        <StatusIcon type={config.icon} color={config.color} />
      </div>
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: 12, color: "rgba(0, 180, 255, 0.9)" }}>{title}</div>
        {details.length > 0 && (
          <div style={{ fontSize: 10, color: "rgba(0, 180, 255, 0.5)", fontFamily: "var(--font-mono)", marginTop: 2 }}>
            {details.join(" -- ")}
          </div>
        )}
      </div>
      <div style={{
        fontSize: 9, fontFamily: "var(--font-mono)",
        color: `${config.color}, 0.7)`,
        letterSpacing: 1,
        padding: "3px 8px",
        background: `${config.color}, 0.08)`,
        borderRadius: 4,
      }}>
        {config.label}
      </div>
    </div>
  );
});
```

- [ ] **Step 2: Wire StatusCard into MessageRenderer**

In `src/components/chat/MessageRenderer.tsx`, replace the status placeholder:

```tsx
// Add import at top
import StatusCard from "./StatusCard";

// Replace the status placeholder block in the render:
if (block.type === "status" && block.data) {
  return <StatusCard key={i} statusType={block.content} data={block.data as Record<string, unknown>} />;
}
```

- [ ] **Step 3: Commit**

```bash
git add src/components/chat/StatusCard.tsx src/components/chat/MessageRenderer.tsx
git commit -m "feat(chat): add StatusCard action result component"
```

---

### Task 4: Integrate MessageRenderer into ChatMessage

**Files:**
- Modify: `src/components/ChatMessage.tsx`

- [ ] **Step 1: Replace plain text with MessageRenderer**

Replace the entire content of `ChatMessage.tsx`:

```tsx
import { memo } from "react";
import type { ChatMessage as ChatMessageType } from "../lib/types";
import MessageRenderer from "./chat/MessageRenderer";

interface ChatMessageProps { message: ChatMessageType; }

export default memo(function ChatMessage({ message }: ChatMessageProps) {
  const isUser = message.role === "user";
  return (
    <div style={{ ...styles.container, alignItems: isUser ? "flex-end" : "flex-start" }}>
      <div style={styles.label}>{isUser ? "YOU" : "JARVIS"}</div>
      <div style={{ ...styles.bubble, borderColor: isUser ? "rgba(0, 180, 255, 0.2)" : "rgba(0, 180, 255, 0.12)", background: isUser ? "rgba(0, 180, 255, 0.06)" : "rgba(0, 180, 255, 0.02)" }}>
        {isUser ? message.content : <MessageRenderer content={message.content} />}
      </div>
    </div>
  );
})

const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", flexDirection: "column", marginBottom: 14, maxWidth: "85%" },
  label: { color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1.5, marginBottom: 4 },
  bubble: { border: "1px solid", borderRadius: 8, padding: "10px 14px", color: "rgba(0, 180, 255, 0.8)", fontSize: 13, lineHeight: 1.5 },
};
```

- [ ] **Step 2: Verify build**

Run: `npm run build 2>&1 | tail -5`

- [ ] **Step 3: Commit**

```bash
git add src/components/ChatMessage.tsx
git commit -m "feat(chat): integrate MessageRenderer for rich message display"
```

---

### Task 5: ChatPanel -- New Input Bar + Width + Remove Fullscreen Overlay

**Files:**
- Modify: `src/components/ChatPanel.tsx`

- [ ] **Step 1: Update ChatPanel with new input bar design and remove fullscreen overlay**

The key changes:
1. Remove `isFullScreen` prop and fullscreen overlay mode
2. Add `onNavigateToChat` prop for expanding to full view
3. Width from 380 to 440
4. New input bar with rounded container + send button

Replace the `ChatPanelProps` interface and the component:

```tsx
interface ChatPanelProps {
  isOpen: boolean;
  onClose: () => void;
  onNavigateToChat: () => void;
}

export default memo(function ChatPanel({ isOpen, onClose, onNavigateToChat }: ChatPanelProps) {
```

Update the header actions -- replace the fullscreen toggle with an expand button:

```tsx
<button onClick={onNavigateToChat} style={styles.headerBtn} title="Open full view">[&gt;]</button>
```

Remove `const panelStyle = isFullScreen ? styles.fullScreen : styles.overlay` and always use overlay.

Update the form to use the new input bar layout:

```tsx
<form onSubmit={handleSubmit} style={styles.inputForm}>
  <div style={styles.inputBar}>
    <textarea
      ref={inputRef}
      value={input}
      onChange={(e) => {
        setInput(e.target.value);
        e.target.style.height = "auto";
        e.target.style.height = Math.min(e.target.scrollHeight, 150) + "px";
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !e.shiftKey) {
          e.preventDefault();
          handleSubmit(e);
          if (inputRef.current) inputRef.current.style.height = "auto";
        }
      }}
      placeholder="Talk to JARVIS..."
      style={styles.input}
      rows={1}
    />
    <button type="submit" style={styles.sendButton} disabled={!input.trim()}>
      <svg width="14" height="14" viewBox="0 0 14 14">
        <path d="M3 7h8M8 4l3 3-3 3" fill="none" stroke="rgba(0, 180, 255, 0.7)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    </button>
  </div>
</form>
```

Update styles -- change overlay width to 440, remove fullScreen style, add inputBar and sendButton:

```tsx
overlay: { position: "fixed", top: 0, right: 0, width: 440, height: "100%", borderLeft: "1px solid rgba(0, 180, 255, 0.15)", background: "rgba(10, 14, 26, 0.97)", display: "flex", flexDirection: "column", zIndex: 100, userSelect: "text" as const },
inputBar: { display: "flex", alignItems: "flex-end", gap: 8 },
input: { flex: 1, background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.18)", borderRadius: 12, padding: "10px 14px", color: "rgba(0, 180, 255, 0.8)", fontSize: 13, fontFamily: "var(--font-sans)", outline: "none", resize: "none" as const, overflow: "hidden", lineHeight: 1.5, boxSizing: "border-box" as const },
sendButton: { width: 34, height: 34, borderRadius: "50%", background: "rgba(0, 180, 255, 0.12)", border: "1px solid rgba(0, 180, 255, 0.25)", display: "flex", alignItems: "center", justifyContent: "center", cursor: "pointer", flexShrink: 0, boxShadow: "0 0 8px rgba(0, 180, 255, 0.08)" },
```

- [ ] **Step 2: Commit**

```bash
git add src/components/ChatPanel.tsx
git commit -m "feat(chat): redesign input bar, widen to 440px, remove fullscreen overlay"
```

---

### Task 6: Add Chat Full View + Sidebar Nav + Remove Voice Auto-Open

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/components/Sidebar.tsx`

- [ ] **Step 1: Add CHAT nav item to Sidebar**

In `src/components/Sidebar.tsx`, add chat to navItems array after "home":

```tsx
const navItems = [
  { id: "home", label: "HOME", icon: "H" },
  { id: "chat", label: "CHAT", icon: ">" },
  { id: "email", label: "MAIL", icon: "M" },
  { id: "calendar", label: "CAL", icon: "C" },
  { id: "github", label: "GIT", icon: "G" },
  { id: "notion", label: "NOT", icon: "N" },
  { id: "cron", label: "CRON", icon: "T" },
  { id: "settings", label: "SET", icon: "S" },
];
```

- [ ] **Step 2: Update App.tsx**

Changes needed:
1. Remove `chatFullScreen` state and `toggleFullScreen`
2. Remove voice auto-open in the chat-state listener (the `setChatOpen(true)` line)
3. Add `case "chat"` to `renderView()` -- reuse ChatPanel's useChat in a full-page layout
4. Update ChatPanel props (remove isFullScreen, add onNavigateToChat)
5. Add `onNavigateToChat` handler that sets activeView to "chat" and closes overlay

In the chat-state listener, remove auto-open:

```tsx
useEffect(() => {
  const unlistenAi = listen<{ state: "idle" | "thinking" | "speaking" }>("chat-state", (event) => {
    setAiState(event.payload.state);
    // Voice auto-open removed -- sphere provides visual feedback
  });
  return () => { unlistenAi.then((fn) => fn()); };
}, []);
```

Add `renderView` case. The chat full view reuses the `useChat` hook but renders in full-width layout (not the overlay):

```tsx
// In renderView():
case "chat": return <ChatFullView />;
```

Create a simple inline `ChatFullView` component in App.tsx (or extract to a file if preferred):

```tsx
function ChatFullView() {
  const { messages, loading, error, send, clearChat, currentStatus, streamingText } = useChat();
  const [input, setInput] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => { messagesEndRef.current?.scrollIntoView({ behavior: "smooth" }); }, [messages]);
  useEffect(() => { inputRef.current?.focus(); }, []);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (input.trim()) { send(input); setInput(""); }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", padding: 24 }}>
      <div style={{ display: "flex", justifyContent: "space-between", marginBottom: 16 }}>
        <span className="system-text">JARVIS CHAT</span>
        <button onClick={clearChat} style={{ background: "transparent", border: "none", color: "rgba(0, 180, 255, 0.5)", fontFamily: "var(--font-mono)", fontSize: 11, cursor: "pointer" }}>NEW</button>
      </div>
      <div style={{ flex: 1, overflowY: "auto", maxWidth: 700 }}>
        {messages.map((msg, i) => <ChatMessageComponent key={msg.id ?? i} message={msg} />)}
        {loading && streamingText && (
          <div style={{ padding: "12px 14px", color: "rgba(0, 180, 255, 0.85)", fontSize: 13, lineHeight: 1.6, whiteSpace: "pre-wrap" }}>
            {streamingText}<span style={{ color: "rgba(0, 180, 255, 0.5)", animation: "blink 1s step-end infinite" }}>|</span>
          </div>
        )}
        {error && <div style={{ color: "var(--accent-urgent)", fontSize: 12, padding: 8 }}>{error}</div>}
        <div ref={messagesEndRef} />
      </div>
      <form onSubmit={handleSubmit} style={{ maxWidth: 700, paddingTop: 12, borderTop: "1px solid rgba(0, 180, 255, 0.08)" }}>
        <div style={{ display: "flex", alignItems: "flex-end", gap: 8 }}>
          <textarea ref={inputRef} value={input} onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSubmit(e); } }}
            placeholder="Talk to JARVIS..." rows={1}
            style={{ flex: 1, background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.18)", borderRadius: 12, padding: "10px 14px", color: "rgba(0, 180, 255, 0.8)", fontSize: 13, fontFamily: "var(--font-sans)", outline: "none", resize: "none", overflow: "hidden", lineHeight: 1.5, boxSizing: "border-box" }} />
          <button type="submit" style={{ width: 34, height: 34, borderRadius: "50%", background: "rgba(0, 180, 255, 0.12)", border: "1px solid rgba(0, 180, 255, 0.25)", display: "flex", alignItems: "center", justifyContent: "center", cursor: "pointer", flexShrink: 0 }}>
            <svg width="14" height="14" viewBox="0 0 14 14"><path d="M3 7h8M8 4l3 3-3 3" fill="none" stroke="rgba(0, 180, 255, 0.7)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /></svg>
          </button>
        </div>
      </form>
    </div>
  );
}
```

Add necessary imports at top of App.tsx: `import { useChat } from "./hooks/useChat"` and `import ChatMessageComponent from "./components/ChatMessage"`.

The overlay ChatPanel is only rendered when activeView is NOT "chat":

```tsx
{activeView !== "chat" && (
  <ChatPanel isOpen={chatOpen} onClose={closeChat} onNavigateToChat={() => { setChatOpen(false); setActiveView("chat"); }} />
)}
```

- [ ] **Step 3: Verify build**

Run: `npm run build 2>&1 | tail -5`

- [ ] **Step 4: Commit**

```bash
git add src/App.tsx src/components/Sidebar.tsx
git commit -m "feat(chat): add chat full view page, remove voice auto-open"
```

---

## B. Voice Integration into 3D Sphere

### Task 7: Backend -- Mic Amplitude Event Emission

**Files:**
- Modify: `src-tauri/src/voice/mod.rs`

- [ ] **Step 1: Add shared AtomicU32 for mic amplitude to VoiceEngine**

Add to VoiceEngine struct:

```rust
use std::sync::atomic::{AtomicU32, Ordering};

pub struct VoiceEngine {
    // ... existing fields ...
    pub mic_amplitude: Arc<AtomicU32>,
}
```

In VoiceEngine constructor, initialize:

```rust
mic_amplitude: Arc::new(AtomicU32::new(0)),
```

- [ ] **Step 2: Write mic amplitude from audio capture**

In the audio capture path (where audio_router records), compute RMS and store:

```rust
// After capturing PCM samples, compute RMS amplitude
let rms = (samples.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt() as f32;
self.mic_amplitude.store(rms.to_bits(), Ordering::Relaxed);
```

- [ ] **Step 3: Add mic amplitude polling emitter**

Add a cancellation flag to VoiceEngine:

```rust
pub struct VoiceEngine {
    // ... existing fields ...
    pub mic_amplitude: Arc<AtomicU32>,
    mic_emitter_active: Arc<AtomicBool>,
}
```

Initialize in constructor: `mic_emitter_active: Arc::new(AtomicBool::new(false))`.

When voice enters Listening state, spawn a polling task:

```rust
// In start_listening or equivalent
self.mic_emitter_active.store(true, Ordering::Relaxed);
let amplitude = self.mic_amplitude.clone();
let active = self.mic_emitter_active.clone();
let handle = self.app_handle.clone();
tokio::spawn(async move {
    while active.load(Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let amp = f32::from_bits(amplitude.load(Ordering::Relaxed));
        if let Some(ref h) = handle {
            let _ = h.emit("mic-amplitude", serde_json::json!({ "amplitude": amp }));
        }
    }
});
```

When listening stops (in stop_listening or state transition away from Listening):

```rust
self.mic_emitter_active.store(false, Ordering::Relaxed);
```

Add import: `use std::sync::atomic::AtomicBool;`

- [ ] **Step 4: Verify backend compiles**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/voice/mod.rs
git commit -m "feat(voice): emit mic-amplitude events via AtomicU32 polling"
```

---

### Task 8: Frontend -- Pass micAmplitudeRef + Wire Up

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Create micAmplitudeRef and listen for mic-amplitude events**

In App.tsx, add:

```tsx
const micAmplitudeRef = useRef(0);

// In the useEffect with TTS amplitude listener, add:
const unlistenMic = listen<{ amplitude: number }>("mic-amplitude", (event) => {
  micAmplitudeRef.current = event.payload.amplitude;
});
```

Pass to JarvisScene:

```tsx
<JarvisScene
  activityLevel={activityLevel}
  ttsAmplitudeRef={ttsAmplitudeRef}
  micAmplitudeRef={micAmplitudeRef}
  pendingToolCall={pendingToolCall}
  onToolCallConsumed={handleToolCallConsumed}
/>
```

- [ ] **Step 2: Commit**

```bash
git add src/App.tsx
git commit -m "feat(voice): pass micAmplitudeRef to JarvisScene"
```

---

### Task 9: JarvisScene -- Mic Waveform + Color Transitions + Particle Attraction

**Files:**
- Modify: `src/components/3d/JarvisScene.tsx`

This is the largest single task. Changes to the Canvas2D animation loop:

- [ ] **Step 1: Add micAmplitudeRef prop**

Update the component props interface:

```tsx
interface JarvisSceneProps {
  activityLevel: "idle" | "listening" | "processing" | "active";
  ttsAmplitudeRef: React.RefObject<number>;
  micAmplitudeRef: React.RefObject<number>;
  pendingToolCall: string | null;
  onToolCallConsumed: () => void;
}
```

- [ ] **Step 2: Add state color tracking**

Inside the animation loop, add color interpolation refs:

```tsx
// Near other refs at component level
const stateColorRef = useRef({ r: 0, g: 180, b: 255 }); // cyan
const targetColorRef = useRef({ r: 0, g: 180, b: 255 });
```

In the animation frame, before drawing the waveform, update target color based on activityLevel:

```tsx
// Color targets
const colorTargets = {
  idle: { r: 0, g: 180, b: 255 },       // cyan
  listening: { r: 0, g: 180, b: 255 },   // cyan
  processing: { r: 255, g: 180, b: 0 },  // amber
  active: { r: 16, g: 185, b: 129 },     // green
};
targetColorRef.current = colorTargets[activityLevel];
// Lerp current toward target
const c = stateColorRef.current;
const t = targetColorRef.current;
const lerpSpeed = 0.04;
c.r += (t.r - c.r) * lerpSpeed;
c.g += (t.g - c.g) * lerpSpeed;
c.b += (t.b - c.b) * lerpSpeed;
```

Use `stateColorRef.current` for waveform bar colors, core glow, and ring tints.

- [ ] **Step 3: Enable waveform during listening (mic amplitude)**

In the waveform drawing section (around line 549-586), currently the waveform only shows when `speakingAlpha > 0.01`. Add a `listeningAlpha` that crossfades in during listening:

```tsx
// Add alongside speakingAlpha logic
const isListening = activityLevel === "listening";
listeningAlphaRef.current += (isListening ? 0.06 : -0.035) * (listeningAlphaRef.current > 0.01 || isListening ? 1 : 0);
listeningAlphaRef.current = Math.max(0, Math.min(1, listeningAlphaRef.current));

const waveAlpha = Math.max(speakingAlpha, listeningAlphaRef.current);
const amplitude = isListening ? (micAmplitudeRef.current ?? 0) : (ttsAmplitudeRef.current ?? 0);
```

Use `waveAlpha` and `amplitude` in the existing waveform drawing code instead of just `speakingAlpha` and `ttsAmplitudeRef`.

- [ ] **Step 4: Add particle attraction during listening**

In the particle/data node update loop, when activityLevel is "listening", add an inward velocity component:

```tsx
// In the particle position update section
if (activityLevel === "listening") {
  const micAmp = micAmplitudeRef.current ?? 0;
  const attractStrength = 0.15 * micAmp;
  // Move particle slightly toward center
  const dx = cx - node.screenX;
  const dy = cy - node.screenY;
  const dist = Math.sqrt(dx * dx + dy * dy);
  if (dist > 20) {
    node.screenX += (dx / dist) * attractStrength;
    node.screenY += (dy / dist) * attractStrength;
  }
}
```

- [ ] **Step 5: Scale core glow with voice amplitude**

In the core drawing section, modulate the inner glow radius and opacity with amplitude:

```tsx
const micAmp = micAmplitudeRef.current ?? 0;
const ttsAmp = ttsAmplitudeRef.current ?? 0;
const voiceAmp = Math.max(micAmp, ttsAmp);
const coreScale = 1 + voiceAmp * 0.2; // Expand up to 20%

// Apply to core gradient radius
const coreR = CORE_RADIUS * coreScale;
```

Use `stateColorRef.current` for the core gradient color.

- [ ] **Step 6: Verify build**

Run: `npm run build 2>&1 | tail -5`

- [ ] **Step 7: Commit**

```bash
git add src/components/3d/JarvisScene.tsx
git commit -m "feat(voice): integrate mic waveform, color transitions, particle attraction into 3D sphere"
```

---

### Task 10: Minimal VoiceIndicator

**Files:**
- Modify: `src/components/VoiceIndicator.tsx`

- [ ] **Step 1: Shrink VoiceIndicator to minimal dot + label**

Replace the component with a minimal version. Keep the same state detection logic but reduce the visual output:

```tsx
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
```

- [ ] **Step 2: Commit**

```bash
git add src/components/VoiceIndicator.tsx
git commit -m "feat(voice): shrink VoiceIndicator to minimal dot + label"
```

---

## C. Natural Language Cron Scheduling

### Task 11: Database Migration + Backend Updates

**Files:**
- Create: `src-tauri/migrations/V6__cron_description.sql`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/commands/cron.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create V6 migration**

```sql
-- src-tauri/migrations/V6__cron_description.sql
ALTER TABLE cron_jobs ADD COLUMN description TEXT;
```

- [ ] **Step 2: Add cron as explicit dependency in Cargo.toml**

Add to `[dependencies]`:

```toml
cron = "0.12"
```

- [ ] **Step 3: Update CronJobView struct in cron.rs**

Add `description` field to the struct:

```rust
#[derive(serde::Serialize)]
pub struct CronJobView {
    pub id: i64,
    pub name: String,
    pub schedule: String,
    pub action_type: String,
    pub status: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub description: Option<String>,
}
```

Update the `get_cron_jobs` SELECT query to include `description`:

```rust
"SELECT id, name, schedule, action_type, status, last_run, next_run, description FROM cron_jobs ORDER BY id"
```

And the row mapping to include `description: row.get(7)?`.

- [ ] **Step 4: Update create_custom_cron AI prompt to return description**

Update the prompt string in `create_custom_cron`:

```rust
let prompt = format!(
    "Parse this scheduling request into a JSON object. \
     Supported action_types: email_sync, calendar_sync, deadline_monitor, notion_sync, github_digest, auto_archive_emails. \
     Return ONLY valid JSON: \
     {{\"name\": \"short name\", \"schedule\": \"cron expression (6-field: sec min hour day month weekday)\", \
     \"action_type\": \"one of the supported types\", \"description\": \"human-readable schedule like 'Every Friday at midnight'\"}} \
     Request: \"{}\"",
    description
);
```

Update the INSERT statement to include `description`:

```rust
db.conn.lock().unwrap().execute(
    "INSERT INTO cron_jobs (name, schedule, action_type, status, description) VALUES (?1, ?2, ?3, 'active', ?4)",
    rusqlite::params![name, schedule, action_type, desc],
)?;
```

Also update the `Ok(CronJobView { ... })` return block to include the new field:

```rust
Ok(CronJobView {
    id: db.conn.lock().unwrap().last_insert_rowid(),
    name: name.to_string(),
    schedule: schedule.to_string(),
    action_type: action_type.to_string(),
    status: "active".to_string(),
    last_run: None,
    next_run: None,
    description: Some(desc.to_string()),
})
```

- [ ] **Step 5: Add get_upcoming_runs command**

```rust
use cron::Schedule;
use std::str::FromStr;
use chrono::Local;

#[tauri::command]
pub fn get_upcoming_runs(schedule: String, count: Option<usize>) -> Result<Vec<String>, String> {
    // Prepend seconds field if 5-field expression
    let expr = if schedule.split_whitespace().count() == 5 {
        format!("0 {}", schedule)
    } else {
        schedule
    };

    let schedule = Schedule::from_str(&expr).map_err(|e| format!("Invalid cron: {}", e))?;
    let n = count.unwrap_or(3);
    let runs: Vec<String> = schedule
        .upcoming(Local)
        .take(n)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .collect();
    Ok(runs)
}
```

- [ ] **Step 6: Register get_upcoming_runs in lib.rs**

Add `cron::get_upcoming_runs` to the `invoke_handler` macro call in `lib.rs`.

- [ ] **Step 7: Verify backend compiles**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`

- [ ] **Step 8: Commit**

```bash
git add src-tauri/migrations/V6__cron_description.sql src-tauri/Cargo.toml src-tauri/src/commands/cron.rs src-tauri/src/lib.rs
git commit -m "feat(cron): add description field, upcoming runs command, V6 migration"
```

---

### Task 12: Frontend Types + Commands

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/commands.ts`

- [ ] **Step 1: Update CronJobView type**

In `src/lib/types.ts`, update:

```typescript
export interface CronJobView {
  id: number;
  name: string;
  schedule: string;
  action_type: string;
  status: string;
  last_run: string | null;
  next_run: string | null;
  description: string | null;
}
```

- [ ] **Step 2: Add getUpcomingRuns command wrapper**

In `src/lib/commands.ts`, add:

```typescript
export async function getUpcomingRuns(schedule: string, count?: number): Promise<string[]> {
  return invoke("get_upcoming_runs", { schedule, count });
}
```

- [ ] **Step 3: Commit**

```bash
git add src/lib/types.ts src/lib/commands.ts
git commit -m "feat(cron): update CronJobView type and add getUpcomingRuns command"
```

---

### Task 13: CronJobCard Component

**Files:**
- Create: `src/components/cron/CronJobCard.tsx`

- [ ] **Step 1: Create CronJobCard**

```tsx
// src/components/cron/CronJobCard.tsx
import { memo, useEffect, useState } from "react";
import type { CronJobView } from "../../lib/types";
import { getUpcomingRuns } from "../../lib/commands";

interface Props {
  job: CronJobView;
  isSelected: boolean;
  onSelect: () => void;
  onToggle: () => void;
  onDelete: () => void;
}

export default memo(function CronJobCard({ job, isSelected, onSelect, onToggle, onDelete }: Props) {
  const [nextRun, setNextRun] = useState<string | null>(null);

  useEffect(() => {
    if (job.status === "active") {
      getUpcomingRuns(job.schedule, 1).then(runs => {
        if (runs.length > 0) setNextRun(runs[0]);
      }).catch(() => {});
    }
  }, [job.schedule, job.status]);

  const isActive = job.status === "active";

  return (
    <div onClick={onSelect} style={{ ...styles.card, ...(isSelected ? styles.cardSelected : {}) }}>
      <div style={styles.header}>
        <div style={{ ...styles.dot, background: isActive ? "rgba(16, 185, 129, 0.7)" : "rgba(255, 100, 100, 0.7)" }} />
        <span style={styles.name}>{job.name}</span>
      </div>
      <div style={styles.schedule}>{job.description || job.schedule}</div>
      {job.description && (
        <div style={styles.cron}>{job.schedule}</div>
      )}
      {nextRun && (
        <div style={styles.nextRun}>Next: {nextRun}</div>
      )}
      {isSelected && (
        <div style={styles.actions}>
          <button onClick={(e) => { e.stopPropagation(); onToggle(); }} style={styles.actionBtn}>
            {isActive ? "PAUSE" : "RESUME"}
          </button>
          <button onClick={(e) => { e.stopPropagation(); onDelete(); }} style={{ ...styles.actionBtn, color: "rgba(255, 100, 100, 0.7)" }}>
            DELETE
          </button>
        </div>
      )}
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  card: { background: "rgba(0, 180, 255, 0.02)", border: "1px solid rgba(0, 180, 255, 0.1)", borderRadius: 8, padding: 12, cursor: "pointer", transition: "border-color 0.2s" },
  cardSelected: { borderColor: "rgba(0, 180, 255, 0.3)", background: "rgba(0, 180, 255, 0.04)" },
  header: { display: "flex", alignItems: "center", gap: 8, marginBottom: 6 },
  dot: { width: 6, height: 6, borderRadius: "50%", flexShrink: 0 },
  name: { fontSize: 13, color: "rgba(0, 180, 255, 0.9)", fontWeight: 500 },
  schedule: { fontSize: 11, color: "rgba(0, 180, 255, 0.6)", marginBottom: 2 },
  cron: { fontSize: 10, color: "rgba(0, 180, 255, 0.3)", fontFamily: "var(--font-mono)" },
  nextRun: { fontSize: 10, color: "rgba(0, 180, 255, 0.4)", fontFamily: "var(--font-mono)", marginTop: 4 },
  actions: { display: "flex", gap: 8, marginTop: 8 },
  actionBtn: { background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "3px 8px", color: "rgba(0, 180, 255, 0.6)", fontFamily: "var(--font-mono)", fontSize: 9, cursor: "pointer", letterSpacing: 1 },
};
```

- [ ] **Step 2: Commit**

```bash
git add src/components/cron/CronJobCard.tsx
git commit -m "feat(cron): add CronJobCard component with human-readable schedule"
```

---

### Task 14: CronTimeline Component

**Files:**
- Create: `src/components/cron/CronTimeline.tsx`

- [ ] **Step 1: Create CronTimeline**

```tsx
// src/components/cron/CronTimeline.tsx
import { memo, useEffect, useState } from "react";
import type { CronRunView } from "../../lib/types";
import { getCronRuns, getUpcomingRuns } from "../../lib/commands";

interface Props {
  jobId: number;
  schedule: string;
}

function formatRelative(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = date.getTime() - now.getTime();
  const diffDays = Math.ceil(diffMs / (1000 * 60 * 60 * 24));
  if (diffDays === 0) return "today";
  if (diffDays === 1) return "in 1d";
  return `in ${diffDays}d`;
}

export default memo(function CronTimeline({ jobId, schedule }: Props) {
  const [upcoming, setUpcoming] = useState<string[]>([]);
  const [runs, setRuns] = useState<CronRunView[]>([]);

  useEffect(() => {
    getUpcomingRuns(schedule, 3).then(setUpcoming).catch(() => {});
    getCronRuns(jobId, 5).then(setRuns).catch(() => {});
  }, [jobId, schedule]);

  return (
    <div style={styles.container}>
      <div style={styles.column}>
        <div style={styles.sectionLabel}>UPCOMING RUNS</div>
        {upcoming.map((run, i) => (
          <div key={i}>
            <div style={{ display: "flex", alignItems: "center", gap: 10, padding: i === 0 ? "8px 10px" : "6px 10px", ...(i === 0 ? styles.nextRun : {}) }}>
              <div style={{ width: i === 0 ? 8 : 6, height: i === 0 ? 8 : 6, borderRadius: "50%", background: `rgba(0, 180, 255, ${0.9 - i * 0.3})`, flexShrink: 0, ...(i === 0 ? { boxShadow: "0 0 8px rgba(0, 180, 255, 0.5)" } : {}) }} />
              <span style={{ fontFamily: "var(--font-mono)", fontSize: i === 0 ? 12 : 11, color: `rgba(0, 180, 255, ${0.9 - i * 0.25})`, flex: 1 }}>{run}</span>
              <span style={{ fontSize: 10, color: `rgba(0, 180, 255, ${0.6 - i * 0.15})`, fontFamily: "var(--font-mono)" }}>{formatRelative(run)}</span>
            </div>
            {i < upcoming.length - 1 && <div style={styles.connector} />}
          </div>
        ))}
      </div>
      <div style={styles.column}>
        <div style={styles.sectionLabel}>RECENT RUNS</div>
        {runs.map((run) => (
          <div key={run.id} style={styles.runRow}>
            <div style={{ fontSize: 9, fontFamily: "var(--font-mono)", padding: "2px 6px", borderRadius: 3, background: run.status === "completed" ? "rgba(16, 185, 129, 0.1)" : "rgba(255, 100, 100, 0.1)", color: run.status === "completed" ? "rgba(16, 185, 129, 0.7)" : "rgba(255, 100, 100, 0.7)", letterSpacing: 0.5 }}>
              {run.status === "completed" ? "DONE" : "FAIL"}
            </div>
            <span style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "rgba(0, 180, 255, 0.5)" }}>{run.started_at}</span>
            <span style={{ fontSize: 10, color: run.error ? "rgba(255, 100, 100, 0.4)" : "rgba(0, 180, 255, 0.3)", flex: 1, textAlign: "right" as const }}>{run.error || run.result || ""}</span>
          </div>
        ))}
        {runs.length === 0 && <div style={{ fontSize: 11, color: "rgba(0, 180, 255, 0.3)", fontStyle: "italic" }}>No runs yet</div>}
      </div>
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", gap: 20, padding: 16 },
  column: { flex: 1 },
  sectionLabel: { fontSize: 10, fontFamily: "var(--font-mono)", color: "rgba(0, 180, 255, 0.4)", letterSpacing: 1.5, marginBottom: 12 },
  nextRun: { background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6 },
  connector: { width: 1, height: 10, background: "rgba(0, 180, 255, 0.15)", marginLeft: 14 },
  runRow: { display: "flex", alignItems: "center", gap: 8, padding: "6px 10px", background: "rgba(0, 180, 255, 0.02)", borderRadius: 4, marginBottom: 4 },
};
```

- [ ] **Step 2: Commit**

```bash
git add src/components/cron/CronTimeline.tsx
git commit -m "feat(cron): add CronTimeline component with upcoming and recent runs"
```

---

### Task 15: ConversionFlow Component

**Files:**
- Create: `src/components/cron/ConversionFlow.tsx`

- [ ] **Step 1: Create ConversionFlow with 4-phase animation**

```tsx
// src/components/cron/ConversionFlow.tsx
import { memo, useState, useCallback } from "react";
import type { CronJobView } from "../../lib/types";
import { createCustomCron } from "../../lib/commands";

interface Props {
  onJobCreated: (job: CronJobView) => void;
}

type Phase = "idle" | "glowing" | "parsing" | "result" | "done" | "error";

export default memo(function ConversionFlow({ onJobCreated }: Props) {
  const [input, setInput] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [result, setResult] = useState<CronJobView | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = useCallback(async () => {
    if (!input.trim() || phase !== "idle") return;

    // Phase 1: glow
    setPhase("glowing");
    await new Promise(r => setTimeout(r, 300));

    // Phase 2: parsing
    setPhase("parsing");

    try {
      const job = await createCustomCron(input);
      setResult(job);

      // Phase 3: result reveal
      setPhase("result");
      await new Promise(r => setTimeout(r, 500));

      // Phase 4: done
      setPhase("done");
      onJobCreated(job);

      // Reset after showing
      setTimeout(() => {
        setPhase("idle");
        setInput("");
        setResult(null);
      }, 2000);

    } catch (e) {
      setError(String(e));
      setPhase("error");
      setTimeout(() => {
        setPhase("idle");
        setError(null);
      }, 3000);
    }
  }, [input, phase, onJobCreated]);

  const inputGlow = phase === "glowing" || phase === "parsing";

  return (
    <div style={styles.container}>
      {/* Input */}
      <div style={styles.inputSection}>
        <div style={styles.inputLabel}>NATURAL LANGUAGE INPUT</div>
        <div style={{ ...styles.inputWrapper, ...(inputGlow ? styles.inputGlow : {}) }}>
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSubmit()}
            placeholder="e.g. Every Monday check email for spam..."
            style={styles.input}
            disabled={phase !== "idle"}
          />
          <button onClick={handleSubmit} style={styles.createBtn} disabled={phase !== "idle" || !input.trim()}>
            + NEW JOB
          </button>
        </div>
      </div>

      {/* Parsing indicator */}
      {phase === "parsing" && (
        <div style={styles.flowSection}>
          <div style={styles.arrow} />
          <div style={styles.parsingLabel} className="animate-glow">AI PARSING</div>
          <div style={styles.arrow} />
        </div>
      )}

      {/* Result */}
      {(phase === "result" || phase === "done") && result && (
        <div style={styles.flowSection}>
          <div style={styles.arrow} />
          <div style={styles.resultRow}>
            <div style={styles.resultCard}>
              <div style={styles.resultLabel}>CRON EXPRESSION</div>
              <div style={styles.cronExpr}>{result.schedule}</div>
            </div>
            <div style={styles.resultCard}>
              <div style={styles.resultLabel}>SCHEDULE</div>
              <div style={styles.scheduleText}>{result.description || result.schedule}</div>
            </div>
          </div>
          {phase === "done" && (
            <>
              <div style={styles.arrow} />
              <div style={styles.doneCard}>
                <svg width="16" height="16" viewBox="0 0 16 16" style={{ flexShrink: 0 }}>
                  <polyline points="4,8 7,11 12,5" fill="none" stroke="rgba(16, 185, 129, 0.8)" strokeWidth="2" strokeLinecap="round" />
                </svg>
                <span style={styles.doneName}>{result.name}</span>
                <span style={styles.doneBadge}>ACTIVE</span>
              </div>
            </>
          )}
        </div>
      )}

      {/* Error */}
      {phase === "error" && error && (
        <div style={styles.flowSection}>
          <div style={styles.arrow} />
          <div style={styles.errorCard}>{error}</div>
        </div>
      )}
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", flexDirection: "column", alignItems: "center", gap: 0, padding: "16px 0" },
  inputSection: { width: "100%", maxWidth: 500 },
  inputLabel: { fontSize: 9, fontFamily: "var(--font-mono)", color: "rgba(0, 180, 255, 0.4)", letterSpacing: 1, marginBottom: 6 },
  inputWrapper: { display: "flex", gap: 8, transition: "box-shadow 0.3s, border-color 0.3s" },
  inputGlow: { filter: "drop-shadow(0 0 8px rgba(0, 180, 255, 0.2))" },
  input: { flex: 1, background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.2)", borderRadius: 8, padding: "10px 14px", color: "rgba(0, 180, 255, 0.9)", fontSize: 13, fontFamily: "var(--font-sans)", outline: "none" },
  createBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.25)", borderRadius: 6, padding: "8px 14px", color: "rgba(0, 180, 255, 0.7)", fontFamily: "var(--font-mono)", fontSize: 10, cursor: "pointer", letterSpacing: 1, whiteSpace: "nowrap" as const },
  flowSection: { display: "flex", flexDirection: "column", alignItems: "center", gap: 6, padding: "4px 0" },
  arrow: { width: 2, height: 16, background: "linear-gradient(to bottom, rgba(0,180,255,0.3), rgba(0,180,255,0.6))" },
  parsingLabel: { padding: "4px 12px", background: "rgba(0, 180, 255, 0.06)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 12, fontFamily: "var(--font-mono)", fontSize: 9, color: "rgba(0, 180, 255, 0.6)", letterSpacing: 1 },
  resultRow: { display: "flex", gap: 12, width: "100%", maxWidth: 500 },
  resultCard: { flex: 1, background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.2)", borderRadius: 8, padding: 12, textAlign: "center" as const },
  resultLabel: { fontSize: 9, fontFamily: "var(--font-mono)", color: "rgba(0, 180, 255, 0.4)", letterSpacing: 1, marginBottom: 6 },
  cronExpr: { fontFamily: "var(--font-mono)", fontSize: 18, color: "rgba(0, 180, 255, 0.95)", letterSpacing: 3, textShadow: "0 0 8px rgba(0, 180, 255, 0.3)" },
  scheduleText: { fontSize: 13, color: "rgba(0, 180, 255, 0.8)" },
  doneCard: { display: "flex", alignItems: "center", gap: 10, padding: "10px 16px", background: "rgba(16, 185, 129, 0.05)", border: "1px solid rgba(16, 185, 129, 0.2)", borderRadius: 8, maxWidth: 500, width: "100%" },
  doneName: { flex: 1, fontSize: 13, color: "rgba(0, 180, 255, 0.9)" },
  doneBadge: { fontSize: 9, fontFamily: "var(--font-mono)", color: "rgba(16, 185, 129, 0.7)", letterSpacing: 1, padding: "3px 8px", background: "rgba(16, 185, 129, 0.08)", borderRadius: 4 },
  errorCard: { padding: "10px 16px", background: "rgba(255, 100, 100, 0.05)", border: "1px solid rgba(255, 100, 100, 0.2)", borderRadius: 8, color: "rgba(255, 100, 100, 0.8)", fontSize: 12, maxWidth: 500, width: "100%" },
};
```

- [ ] **Step 2: Commit**

```bash
git add src/components/cron/ConversionFlow.tsx
git commit -m "feat(cron): add ConversionFlow component with 4-phase animation"
```

---

### Task 16: CronDashboard -- New Layout

**Files:**
- Modify: `src/pages/CronDashboard.tsx`

- [ ] **Step 1: Rewrite CronDashboard with new vertical layout**

Replace the entire component. Key changes:
- Remove `window.location.reload()` calls -- use state re-fetching
- Top: ConversionFlow
- Middle: Job cards grid
- Bottom: CronTimeline (when job selected)

```tsx
// src/pages/CronDashboard.tsx
import { useState, useEffect, useCallback } from "react";
import type { CronJobView } from "../lib/types";
import { getCronJobs, toggleCronJob, deleteCronJob } from "../lib/commands";
import ConversionFlow from "../components/cron/ConversionFlow";
import CronJobCard from "../components/cron/CronJobCard";
import CronTimeline from "../components/cron/CronTimeline";

export default function CronDashboard() {
  const [jobs, setJobs] = useState<CronJobView[]>([]);
  const [selectedJobId, setSelectedJobId] = useState<number | null>(null);

  const fetchJobs = useCallback(() => {
    getCronJobs().then(setJobs).catch(console.error);
  }, []);

  useEffect(() => { fetchJobs(); }, [fetchJobs]);

  const selectedJob = jobs.find(j => j.id === selectedJobId);

  const handleJobCreated = useCallback((job: CronJobView) => {
    setJobs(prev => [...prev, job]);
    setSelectedJobId(job.id);
  }, []);

  const handleToggle = useCallback(async (id: number) => {
    await toggleCronJob(id);
    fetchJobs();
  }, [fetchJobs]);

  const handleDelete = useCallback(async (id: number) => {
    await deleteCronJob(id);
    if (selectedJobId === id) setSelectedJobId(null);
    fetchJobs();
  }, [selectedJobId, fetchJobs]);

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <span className="system-text">CRON SCHEDULING</span>
      </div>

      {/* Top: Conversion Flow */}
      <ConversionFlow onJobCreated={handleJobCreated} />

      {/* Middle: Job Cards Grid */}
      <div style={styles.grid}>
        {jobs.map(job => (
          <CronJobCard
            key={job.id}
            job={job}
            isSelected={selectedJobId === job.id}
            onSelect={() => setSelectedJobId(selectedJobId === job.id ? null : job.id)}
            onToggle={() => handleToggle(job.id)}
            onDelete={() => handleDelete(job.id)}
          />
        ))}
        {jobs.length === 0 && (
          <div style={styles.empty}>No cron jobs yet. Create one above.</div>
        )}
      </div>

      {/* Bottom: Timeline (when job selected) */}
      {selectedJob && (
        <div style={styles.timeline}>
          <div style={styles.timelineHeader}>
            <span className="system-text">{selectedJob.name.toUpperCase()}</span>
          </div>
          <CronTimeline jobId={selectedJob.id} schedule={selectedJob.schedule} />
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: { padding: 24, maxWidth: 800, margin: "0 auto" },
  header: { marginBottom: 8 },
  grid: { display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))", gap: 12, padding: "16px 0" },
  empty: { color: "rgba(0, 180, 255, 0.3)", fontSize: 12, fontStyle: "italic", gridColumn: "1 / -1", textAlign: "center" as const, padding: 40 },
  timeline: { marginTop: 8, background: "rgba(0, 180, 255, 0.02)", border: "1px solid rgba(0, 180, 255, 0.1)", borderRadius: 8 },
  timelineHeader: { padding: "12px 16px", borderBottom: "1px solid rgba(0, 180, 255, 0.08)" },
};
```

- [ ] **Step 2: Verify full build**

Run: `npm run build 2>&1 | tail -10`

- [ ] **Step 3: Commit**

```bash
git add src/pages/CronDashboard.tsx
git commit -m "feat(cron): redesign CronDashboard with conversion flow, grid, and timeline"
```

---

### Task 17: Backend -- AI Tool Definitions for Charts/Status

**Files:**
- Modify: `src-tauri/src/ai/tools.rs`

- [ ] **Step 1: Add render_chart and render_status tool definitions**

Add these to the `get_tool_definitions()` function:

```rust
Tool {
    name: "render_chart".to_string(),
    description: "Render a data chart inline in the chat. Use when presenting numerical data, trends, or comparisons.".to_string(),
    parameters: serde_json::json!({
        "type": "object",
        "properties": {
            "chart_type": {
                "type": "string",
                "enum": ["line", "pie", "bar"],
                "description": "Type of chart to render"
            },
            "data": {
                "type": "object",
                "description": "Chart data. For line/bar: {labels: string[], series: [{name: string, data: number[]}]}. For pie: {segments: [{label: string, value: number}]}"
            }
        },
        "required": ["chart_type", "data"]
    }),
},
Tool {
    name: "render_status".to_string(),
    description: "Show a status card for a completed action. Use after creating tasks, syncing data, or completing any action.".to_string(),
    parameters: serde_json::json!({
        "type": "object",
        "properties": {
            "status_type": {
                "type": "string",
                "enum": ["task_created", "task_completed", "email_synced", "calendar_synced", "cron_created", "action_completed"],
                "description": "Type of status to display"
            },
            "data": {
                "type": "object",
                "description": "Status data. Fields vary by type: task_created needs name/priority/due, email_synced needs count/folder, etc."
            }
        },
        "required": ["status_type", "data"]
    }),
},
```

- [ ] **Step 2: Handle render_chart and render_status in tools.rs execute_tool()**

In `src-tauri/src/ai/tools.rs`, in the `execute_tool` match block (where all other tools like `open_app`, `create_task` etc. are matched), add cases for the new tools. These tools don't execute external actions -- they serialize data as tags for the frontend to render:

```rust
"render_chart" => {
    let chart_type = args["chart_type"].as_str().unwrap_or("line");
    let data = &args["data"];
    Ok(format!("[CHART:{}|{}]", chart_type, serde_json::to_string(data).unwrap_or_default()))
}
"render_status" => {
    let status_type = args["status_type"].as_str().unwrap_or("action_completed");
    let data = &args["data"];
    Ok(format!("[STATUS:{}|{}]", status_type, serde_json::to_string(data).unwrap_or_default()))
}
```

- [ ] **Step 3: Verify backend compiles**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/ai/tools.rs
git commit -m "feat(ai): add render_chart and render_status tool definitions for inline display"
```

---

### Task 18: Final Build Verification

- [ ] **Step 1: Full frontend build**

Run: `npm run build 2>&1 | tail -10`
Expected: Build succeeds with no errors.

- [ ] **Step 2: Full backend build**

Run: `cd src-tauri && cargo build 2>&1 | tail -10`
Expected: Build succeeds with no errors.

- [ ] **Step 3: Launch and smoke test**

Run: `npm run tauri dev`

Test checklist:
- Chat overlay opens from sidebar, shows at 440px width
- Chat full view accessible from sidebar CHAT nav item
- Markdown renders (bold, lists) in assistant messages
- Voice indicator shows minimal dot during listening
- 3D sphere responds to voice state changes
- Cron dashboard shows conversion flow animation
- Creating a cron job shows animated parsing flow
- Selected cron job shows timeline with upcoming runs

- [ ] **Step 4: Final commit if any fixes needed**

```bash
git add -A
git commit -m "fix: address build issues from UIUX upgrade"
```
