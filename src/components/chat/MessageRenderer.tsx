import { memo } from "react";

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

    // Empty line
    if (trimmed === "") {
      flushList();
      elements.push(<div key={key++} style={{ height: 6 }} />);
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
        if (block.type === "chart") {
          return (
            <div key={i} style={placeholderStyle}>
              [Chart: {block.content}]
            </div>
          );
        }

        if (block.type === "status") {
          return (
            <div key={i} style={placeholderStyle}>
              [Status: {block.content}]
            </div>
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
  fontSize: 13,
  fontFamily: "var(--font-sans)",
  color: "rgba(0, 180, 255, 0.8)",
};

const placeholderStyle: React.CSSProperties = {
  padding: "8px 12px",
  marginTop: 6,
  background: "rgba(0, 180, 255, 0.04)",
  border: "1px solid rgba(0, 180, 255, 0.1)",
  borderRadius: 6,
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  color: "rgba(0, 180, 255, 0.5)",
};

const inlineStyles: Record<string, React.CSSProperties> = {
  bold: {
    fontWeight: 600,
    color: "rgba(0, 180, 255, 0.95)",
  },
  italic: {
    fontStyle: "italic",
    color: "rgba(0, 180, 255, 0.75)",
  },
  code: {
    fontFamily: "var(--font-mono)",
    fontSize: 12,
    background: "rgba(0, 180, 255, 0.08)",
    border: "1px solid rgba(0, 180, 255, 0.12)",
    borderRadius: 3,
    padding: "1px 5px",
    color: "rgba(0, 180, 255, 0.9)",
  },
  link: {
    color: "rgba(0, 180, 255, 0.95)",
    textDecoration: "underline",
    textDecorationColor: "rgba(0, 180, 255, 0.3)",
  },
};

const mdStyles: Record<string, React.CSSProperties> = {
  h1: {
    fontSize: 17,
    fontWeight: 600,
    color: "rgba(0, 180, 255, 0.95)",
    marginBottom: 6,
    marginTop: 8,
    fontFamily: "var(--font-sans)",
  },
  h2: {
    fontSize: 15,
    fontWeight: 600,
    color: "rgba(0, 180, 255, 0.9)",
    marginBottom: 4,
    marginTop: 6,
    fontFamily: "var(--font-sans)",
  },
  h3: {
    fontSize: 14,
    fontWeight: 600,
    color: "rgba(0, 180, 255, 0.85)",
    marginBottom: 4,
    marginTop: 4,
    fontFamily: "var(--font-sans)",
  },
  paragraph: {
    marginBottom: 2,
  },
  list: {
    paddingLeft: 18,
    marginBottom: 4,
    marginTop: 2,
  },
  listItem: {
    marginBottom: 2,
    color: "rgba(0, 180, 255, 0.8)",
  },
};
