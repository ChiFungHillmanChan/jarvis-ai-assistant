export interface MailText { type: 'text'; text: string }
export interface MailQuote { type: 'quote'; attribution: string | null; parts: MailPart[] }
export type MailPart = MailText | MailQuote;

export interface QuoteAuthor { name: string; address: string | null; date: string }

// A quote marker is ">" followed by a space, another ">" or the end of the line,
// so text such as ">50% of users" stays as written.
const QUOTED = /^>(?=[ \t>]|$)/;
const WROTE = /(?:\bwrote|[寫写]道)\s*[:：]\s*$/i;

/** Splits a plain-text email into its own text and the ">" quoted messages under it. */
export function splitQuotes(text: string): MailPart[] {
  return parse(text.split(/\r?\n/));
}

function parse(lines: string[]): MailPart[] {
  const parts: MailPart[] = [];
  let pending: string[] = [];
  const flush = () => {
    const text = pending.join('\n').replace(/^(?:[ \t]*\n)+|(?:\n[ \t]*)+$/g, '');
    if (text.trim()) parts.push({ type: 'text', text });
    pending = [];
  };
  for (let i = 0; i < lines.length;) {
    if (!QUOTED.test(lines[i])) { pending.push(lines[i++]); continue; }
    const quoted: string[] = [];
    while (i < lines.length && QUOTED.test(lines[i])) quoted.push(lines[i++].replace(/^>[ \t]?/, ''));
    const attribution = takeAttribution(pending);
    flush();
    parts.push({ type: 'quote', attribution, parts: parse(quoted) });
  }
  flush();
  return parts;
}

// Removes and returns the "On <date>, <name> wrote:" line that introduces a quote.
// Gmail wraps long ones, so look back up to three unbroken lines for where it starts.
function takeAttribution(lines: string[]): string | null {
  let end = lines.length;
  while (end > 0 && !lines[end - 1].trim()) end--;
  if (!end || !WROTE.test(lines[end - 1])) return null;
  let start = end - 1;
  for (let k = end - 1; k >= Math.max(0, end - 3) && lines[k].trim(); k--) {
    if (/^\s*On\s/i.test(lines[k])) { start = k; break; }
    if (lines[k].includes('@')) start = k;
  }
  return lines.splice(start).slice(0, end - start).join(' ').replace(/\s+/g, ' ').trim();
}

const EN_AUTHOR = /^On (.+), ("[^"]+"|[^,"]+?)(?: <([^<>\s]+@[^<>\s]+)>)? wrote:$/i;
const ZH_AUTHOR = /^(.+?)\s*(?:<([^<>\s]+@[^<>\s]+)>)?\s*[於于]\s*(.+?)\s*[寫写]道\s*[:：]$/;

/** Reads the sender and date from Gmail, Apple Mail and Outlook attribution lines. */
export function quoteAuthor(attribution: string): QuoteAuthor | null {
  const line = attribution.replace(/\s+/g, ' ').trim();
  const en = EN_AUTHOR.exec(line);
  if (en) return author(en[2], en[3], en[1]);
  const zh = ZH_AUTHOR.exec(line);
  return zh ? author(zh[1], zh[2], zh[3]) : null;
}

function author(name: string, address: string | undefined, date: string): QuoteAuthor {
  const bare = /^<([^<>\s]+@[^<>\s]+)>$/.exec(name);
  if (bare) return { name: bare[1], address: bare[1], date };
  return { name: name.replace(/^"(.*)"$/, '$1'), address: address ?? null, date };
}

/**
 * Top-posted history nests each earlier message inside the next. List them newest
 * first instead; a quote without its own attribution is part of the message around it.
 */
export function quoteThread(quote: MailQuote): MailQuote[] {
  const last = quote.parts[quote.parts.length - 1];
  if (quote.parts.length < 2 || last.type !== 'quote' || !last.attribution) return [quote];
  return [{ ...quote, parts: quote.parts.slice(0, -1) }, ...quoteThread(last)];
}
