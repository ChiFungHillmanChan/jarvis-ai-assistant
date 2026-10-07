import { createElement, useId, useState, type MouseEvent, type ReactElement } from 'react';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { ChevronRight } from 'lucide-react';
import { quoteAuthor, quoteThread, splitQuotes, type MailPart, type MailQuote } from './mail-quotes.ts';

export interface QuoteLabels {
  /** count is the number of earlier messages, or 0 for quoted text without a sender line. */
  show: (count: number) => string;
  hide: string;
}

export interface ContentBodyProps {
  text: string;
  format?: 'markdown' | 'plain';
  sourceIds?: number[];
  onSource?: (id: number) => void;
  onOpenLink?: (url: string) => void;
  tableLabel?: string;
  quoteLabels?: QuoteLabels;
}

const ZH_QUOTE_LABELS: QuoteLabels = { show: n => n ? `顯示較早的 ${n} 封郵件` : '顯示引用內容', hide: '隱藏引用內容' };

function quoteBlock(quote: MailQuote): ReactElement {
  const from = quote.attribution ? quoteAuthor(quote.attribution) : null;
  return createElement('blockquote', { className: 'readable-quote' },
    quote.attribution && createElement('div', { className: 'readable-quote-from' }, ...(from
      ? [createElement('strong', { title: from.address ?? undefined }, from.name), createElement('span', null, from.date)]
      : [quote.attribution])),
    ...mailParts(quote.parts));
}

function mailParts(parts: MailPart[]): ReactElement[] {
  return parts.map(part => part.type === 'text' ? createElement('div', { className: 'readable-mail-text' }, part.text) : quoteBlock(part));
}

// Like Gmail, fold only the history under a reply; quotes answered inline stay open.
function PlainMail({ text, labels }: { text: string; labels: QuoteLabels }) {
  const [openText, setOpenText] = useState<string | null>(null);
  const threadId = useId();
  const parts = splitQuotes(text);
  const last = parts[parts.length - 1];
  const trail = parts.length > 1 && last.type === 'quote' ? last : null;
  if (!trail) return createElement('div', { className: 'readable-content-plain' }, ...mailParts(parts));
  const open = openText === text;
  const thread = quoteThread(trail);
  const from = trail.attribution ? quoteAuthor(trail.attribution) : null;
  return createElement('div', { className: 'readable-content-plain' },
    ...mailParts(parts.slice(0, -1)),
    createElement('button', {
      type: 'button', className: 'readable-quote-toggle', 'aria-expanded': open, 'aria-controls': open ? threadId : undefined,
      onClick: () => setOpenText(open ? null : text),
    },
    createElement(ChevronRight, { size: 14, 'aria-hidden': true }),
    createElement('strong', null, open ? labels.hide : labels.show(trail.attribution ? thread.length : 0)),
    !open && from && createElement('span', null, `${from.name} · ${from.date}`)),
    open && createElement('div', { id: threadId, className: 'readable-quote-thread' }, ...thread.map(quoteBlock)));
}

interface ContentNode {
  type: string;
  value?: string;
  url?: string;
  children?: ContentNode[];
}

// Work on parsed text nodes, so citations in code and existing links remain literal.
function mailCitations(sourceIds: Set<number>) {
  return () => (tree: ContentNode) => {
    function visit(node: ContentNode) {
      if (!node.children || ['link', 'code', 'inlineCode'].includes(node.type)) return;
      node.children = node.children.flatMap(child => {
        if (child.type !== 'text' || !child.value) { visit(child); return [child]; }
        const parts: ContentNode[] = [];
        let last = 0;
        for (const match of child.value.matchAll(/\[郵件\s*#(\d+)\]/g)) {
          const id = Number(match[1]);
          if (!sourceIds.has(id)) continue;
          const start = match.index!;
          if (start > last) parts.push({ type: 'text', value: child.value.slice(last, start) });
          parts.push({ type: 'link', url: `jarvis-source:${id}`, children: [{ type: 'text', value: `郵件 #${id}` }] });
          last = start + match[0].length;
        }
        if (!parts.length) return [child];
        if (last < child.value.length) parts.push({ type: 'text', value: child.value.slice(last) });
        return parts;
      });
    }
    visit(tree);
  };
}

function safeUrl(value: string): string {
  try {
    const url = new URL(value);
    return ['https:', 'http:', 'mailto:'].includes(url.protocol) && !url.username && !url.password ? url.href : '';
  } catch { return ''; }
}

export function ContentBody({ text, format = 'markdown', sourceIds = [], onSource, onOpenLink, tableLabel = '表格，可水平捲動', quoteLabels = ZH_QUOTE_LABELS }: ContentBodyProps) {
  if (format === 'plain') return createElement(PlainMail, { text, labels: quoteLabels });
  const validSources = new Set(sourceIds.filter(id => Number.isSafeInteger(id) && id > 0));
  const sourceId = (url: string) => /^jarvis-source:\d+$/.test(url) && validSources.has(Number(url.slice(14))) ? Number(url.slice(14)) : null;
  // Some models wrap the entire document in a Markdown fence. Only unwrap an
  // explicitly labelled document; actual code samples keep their contents intact.
  const wrapped = text.match(/^\s*(`{3,}|~{3,})(?:markdown|md)\s*\r?\n([\s\S]*)\r?\n\1\s*$/i);
  return createElement(Markdown, {
    children: wrapped ? wrapped[2] : text,
    remarkPlugins: [remarkGfm, mailCitations(onSource ? validSources : new Set())],
    skipHtml: true,
    urlTransform: url => onSource && sourceId(url) !== null ? url : safeUrl(url),
    components: {
      a: ({ href, children, title }) => {
        const id = href ? sourceId(href) : null;
        if (id !== null && onSource) return createElement('button', { type: 'button', className: 'readable-source', 'data-source-id': id, onClick: () => onSource(id) }, children);
        if (!href) return createElement('span', null, children);
        return createElement('a', {
          href, title, target: '_blank', rel: 'noopener noreferrer',
          onClick: onOpenLink ? (event: MouseEvent<HTMLAnchorElement>) => { event.preventDefault(); onOpenLink(href); } : undefined,
        }, children);
      },
      // Display the description without loading remote images or tracking pixels.
      img: ({ alt }) => createElement('span', { className: 'readable-image-label' }, alt || ''),
      table: ({ children }) => createElement('div', { className: 'readable-table', role: 'region', 'aria-label': tableLabel, tabIndex: 0 }, createElement('table', null, children)),
    },
  });
}
