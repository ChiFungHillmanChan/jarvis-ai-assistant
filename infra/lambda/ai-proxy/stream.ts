import type { Usage } from './pricing';

export type ProxyEvent =
  | { type: 'delta'; text: string }
  | { type: 'tool_use'; toolUseId: string; name: string; input: unknown }
  | { type: 'done'; stopReason: string; modelId: string; inputTokens: number; outputTokens: number; billedMicros: number; period: { key: string; usedMicros: number; capMicros: number } }
  | { type: 'error'; code: string };

export interface StreamSummary { stopReason: string | null; usage: Usage | null; streamedChars: number; failed: boolean }

type Held = { kind: 'text'; block: number; text: string } | { kind: 'tool'; event: ProxyEvent };

/**
 * Relays Bedrock ConverseStream events as proxy `delta` and `tool_use` events. Reasoning content is
 * counted but never emitted. Task 5 sends `done` and `error`. Exception messages are never read.
 * In DSML mode (DeepSeek with tools) everything is held until messageStop so leaked DSML text can be
 * dropped from tool-use turns.
 */
export async function relay(
  events: AsyncIterable<Record<string, any>>,
  opts: { dsml: boolean; hasTools: boolean },
  emit: (e: ProxyEvent) => void,
): Promise<StreamSummary> {
  const hold = opts.dsml && opts.hasTools;
  const summary: StreamSummary = { stopReason: null, usage: null, streamedChars: 0, failed: false };
  const toolBlocks = new Map<number, { toolUseId: string; name: string; input: string }>();
  const held: Held[] = [];
  let flushed = false;

  const flush = () => {
    if (flushed) return;
    flushed = true;
    const hasTool = held.some(h => h.kind === 'tool');
    const joined = new Map<number, string>();
    for (const h of held) if (h.kind === 'text') joined.set(h.block, (joined.get(h.block) ?? '') + h.text);
    for (const h of held) {
      if (h.kind === 'tool') emit(h.event);
      else if (!(hasTool && joined.get(h.block)!.includes('DSML'))) emit({ type: 'delta', text: h.text });
    }
  };

  const out = (e: ProxyEvent, block: number) => {
    if (!hold) emit(e);
    else if (e.type === 'delta') held.push({ kind: 'text', block, text: e.text });
    else held.push({ kind: 'tool', event: e });
  };

  try {
    for await (const ev of events) {
      if (Object.keys(ev).some(k => k.endsWith('Exception'))) { summary.failed = true; break; }

      const start = ev.contentBlockStart;
      if (start?.start?.toolUse) {
        toolBlocks.set(start.contentBlockIndex, { toolUseId: start.start.toolUse.toolUseId, name: start.start.toolUse.name, input: '' });
      }

      const d = ev.contentBlockDelta;
      if (d) {
        const delta = d.delta ?? {};
        if (typeof delta.text === 'string' && delta.text.length > 0) {
          summary.streamedChars += delta.text.length;
          out({ type: 'delta', text: delta.text }, d.contentBlockIndex);
        }
        if (typeof delta.toolUse?.input === 'string') {
          summary.streamedChars += delta.toolUse.input.length;
          const tb = toolBlocks.get(d.contentBlockIndex);
          if (tb) tb.input += delta.toolUse.input;
        }
        if (typeof delta.reasoningContent?.text === 'string') summary.streamedChars += delta.reasoningContent.text.length;
      }

      const stop = ev.contentBlockStop;
      if (stop) {
        const tb = toolBlocks.get(stop.contentBlockIndex);
        if (tb) {
          toolBlocks.delete(stop.contentBlockIndex);
          let input: unknown;
          try { input = tb.input === '' ? {} : JSON.parse(tb.input); } catch { summary.failed = true; break; }
          out({ type: 'tool_use', toolUseId: tb.toolUseId, name: tb.name, input }, stop.contentBlockIndex);
        }
      }

      if (ev.messageStop) {
        summary.stopReason = ev.messageStop.stopReason ?? null;
        if (hold) flush();
      }

      const u = ev.metadata?.usage;
      if (u && Number.isFinite(u.inputTokens) && Number.isFinite(u.outputTokens)) {
        summary.usage = { inputTokens: u.inputTokens, outputTokens: u.outputTokens };
      }
    }
  } catch {
    summary.failed = true;
  }
  // A stream that ended cleanly without messageStop still delivers what it held; a failed one does not.
  if (hold && !summary.failed) flush();
  return summary;
}
