import { memo } from "react";
import InlineChart from "./InlineChart";
import StatusCard from "./StatusCard";

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
// Inline markdown renderer
// ---------------------------------------------------------------------------

function renderInline(text: string): (string | JSX.Element)[] {
  const parts: (string | JSX.Element)[] = [];
  // Process inline formatting: bold, italic, code, links
  const inlineRe =
    /(\*\*(.+?)\*\*)|(\*(.+?)\*)|(`([^`]+?)`)|(\[([^\]]+)\]\(([^)]+)\))/g;

  let last = 0;
  let m: RegExpExecArray | null;
  let key = 0;

  while ((m = inlineRe.exec(text)) !== null) {
    if (m.index > last) {
      parts.push(text.slice(last, m.index));
    }

    if (m[1]) {
      // bold
      parts.push(
        <strong key={key++} style={inlineStyles.bold}>
          {m[2]}
        </strong>
      );
    } else if (m[3]) {
      // italic
      parts.push(
        <em key={key++} style={inlineStyles.italic}>
          {m[4]}
        </em>
      );
    } else if (m[5]) {
      // inline code
      parts.push(
        <code key={key++} style={inlineStyles.code}>
          {m[6]}
        </code>
      );
    } else if (m[7]) {
      // link
      parts.push(
        <a
          key={key++}
          href={m[9]}
          target="_blank"
          rel="noopener noreferrer"
          style={inlineStyles.link}
        >
          {m[8]}
        </a>
      );
    }

    last = m.index + m[0].length;
  }

  if (last < text.length) {
    parts.push(text.slice(last));
  }

  return parts;
}

// ---------------------------------------------------------------------------
// Markdown block renderer
// ---------------------------------------------------------------------------

function renderMarkdown(text: string): JSX.Element {
  const lines = text.split("\n");
  const elements: JSX.Element[] = [];
  let key = 0;
  let listItems: JSX.Element[] = [];
  let listType: "ul" | "ol" | null = null;

  const flushList = () => {
    if (listItems.length > 0 && listType) {
      const Tag = listType === "ul" ? "ul" : "ol";
      elements.push(
        <Tag key={key++} style={mdStyles.list}>
          {listItems}
        </Tag>
      );
      listItems = [];
      listType = null;
    }
  };

  for (const line of lines) {
    const trimmed = line.trimStart();

    // Headings
    const headingMatch = trimmed.match(/^(#{1,3})\s+(.+)$/);
    if (headingMatch) {
      flushList();
      const level = headingMatch[1].length as 1 | 2 | 3;
      const headingStyle =
        level === 1
          ? mdStyles.h1
          : level === 2
            ? mdStyles.h2
            : mdStyles.h3;
      elements.push(
        <div key={key++} style={headingStyle}>
          {renderInline(headingMatch[2])}
        </div>
      );
      continue;
    }

    // Unordered list
    const ulMatch = trimmed.match(/^[-*]\s+(.+)$/);
    if (ulMatch) {
      if (listType !== "ul") flushList();
      listType = "ul";
      listItems.push(
        <li key={key++} style={mdStyles.listItem}>
          {renderInline(ulMatch[1])}
        </li>
      );
      continue;
    }

    // Ordered list
    const olMatch = trimmed.match(/^\d+\.\s+(.+)$/);
    if (olMatch) {
      if (listType !== "ol") flushList();
      listType = "ol";
      listItems.push(
        <li key={key++} style={mdStyles.listItem}>
          {renderInline(olMatch[1])}
        </li>
      );
      continue;
    }

    // Empty line -- treat as paragraph separator
    if (trimmed === "") {
      flushList();
      elements.push(<div key={key++} style={{ height: 8 }} />);
      continue;
    }

    // Regular paragraph
    flushList();
    elements.push(
      <div key={key++} style={mdStyles.paragraph}>
        {renderInline(trimmed)}
      </div>
    );
  }

  flushList();

  return <>{elements}</>;
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
          <div key={i}>{renderMarkdown(block.content)}</div>
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

const inlineStyles: Record<string, React.CSSProperties> = {
  bold: {
    fontWeight: 600,
    color: "var(--text-primary)",
  },
  italic: {
    fontStyle: "italic",
    color: "var(--text-secondary)",
  },
  code: {
    fontFamily: "var(--font-mono)",
    fontSize: 12,
    background: "var(--surface-solid)",
    border: "1px solid var(--border-primary)",
    borderRadius: 3,
    padding: "1px 5px",
    color: "var(--text-primary)",
  },
  link: {
    color: "var(--cyan)",
    textDecoration: "underline",
    textDecorationColor: "var(--border-primary)",
  },
};

const mdStyles: Record<string, React.CSSProperties> = {
  h1: {
    fontSize: 17,
    fontWeight: 600,
    color: "var(--text-primary)",
    marginBottom: 6,
    marginTop: 8,
    fontFamily: "var(--font-sans)",
  },
  h2: {
    fontSize: 15,
    fontWeight: 600,
    color: "var(--text-primary)",
    marginBottom: 4,
    marginTop: 6,
    fontFamily: "var(--font-sans)",
  },
  h3: {
    fontSize: 14,
    fontWeight: 600,
    color: "var(--text-primary)",
    marginBottom: 4,
    marginTop: 4,
    fontFamily: "var(--font-sans)",
  },
  paragraph: {
    marginBottom: 10,
    lineHeight: 1.65,
  },
  list: {
    paddingLeft: 20,
    marginBottom: 10,
    marginTop: 4,
  },
  listItem: {
    marginBottom: 4,
    lineHeight: 1.65,
    color: "var(--text-primary)",
  },
};
