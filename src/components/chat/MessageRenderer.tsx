import { memo } from "react";
import InlineChart from "./InlineChart";
import StatusCard from "./StatusCard";
import ReadableContent from "../content/ReadableContent";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface Block {
  type: "text" | "chart" | "status";
  content: string;
  data?: Record<string, unknown>;
}

interface MessageRendererProps {
  content: string;
}

// ---------------------------------------------------------------------------
// Block parser -- splits message into text / chart / status blocks
// ---------------------------------------------------------------------------

const TAG_RE = /\[(CHART|STATUS):([^|]+)\|(.+?)\]/g;

function parseBlocks(raw: string): Block[] {
  const blocks: Block[] = [];
  let lastIndex = 0;

  let match: RegExpExecArray | null;
  while ((match = TAG_RE.exec(raw)) !== null) {
    // Text before the tag
    if (match.index > lastIndex) {
      blocks.push({ type: "text", content: raw.slice(lastIndex, match.index) });
    }

    const kind = match[1].toLowerCase() as "chart" | "status";
    const subType = match[2].trim();
    let data: Record<string, unknown> = {};
    try {
      data = JSON.parse(match[3]);
    } catch {
      // If JSON is invalid, keep empty data
    }

    blocks.push({ type: kind, content: subType, data });
    lastIndex = match.index + match[0].length;
  }

  // Remaining text
  if (lastIndex < raw.length) {
    blocks.push({ type: "text", content: raw.slice(lastIndex) });
  }

  // Reset regex state
  TAG_RE.lastIndex = 0;

  if (blocks.length === 0) {
    blocks.push({ type: "text", content: raw });
  }

  return blocks;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

function MessageRendererInner({ content }: MessageRendererProps) {
  const blocks = parseBlocks(content);

  return (
    <div style={containerStyle}>
      {blocks.map((block, i) => {
        if (block.type === "chart" && block.data) {
          return (
            <InlineChart
              key={i}
              chartType={block.content}
              data={block.data as Record<string, unknown>}
            />
          );
        }

        if (block.type === "status" && block.data) {
          return (
            <StatusCard
              key={i}
              statusType={block.content}
              data={block.data as Record<string, unknown>}
            />
          );
        }

        return (
          <ReadableContent key={i} text={block.content} />
        );
      })}
    </div>
  );
}

const MessageRenderer = memo(MessageRendererInner);
export default MessageRenderer;

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const containerStyle: React.CSSProperties = {
  lineHeight: 1.55,
  fontSize: 14,
  fontFamily: "var(--font-sans)",
  color: "var(--text-primary)",
};
