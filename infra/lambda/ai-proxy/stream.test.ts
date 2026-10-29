import test from 'node:test';
import assert from 'node:assert/strict';
import { loadTs } from '../../test-support/load-ts.cjs';

const { relay } = loadTs('lambda/ai-proxy/stream.ts');

async function* from(list: any[]) { for (const e of list) yield e; }
async function* thenThrow(list: any[]) { for (const e of list) yield e; throw new Error('boom'); }

const usage = { metadata: { usage: { inputTokens: 12, outputTokens: 3, totalTokens: 15 } } };
const text = (t: string, i = 0) => ({ contentBlockDelta: { contentBlockIndex: i, delta: { text: t } } });
const stop = (stopReason: string) => ({ messageStop: { stopReason } });
const toolStart = (i: number, toolUseId: string, name: string) => ({ contentBlockStart: { contentBlockIndex: i, start: { toolUse: { toolUseId, name } } } });
const toolInput = (i: number, input: string) => ({ contentBlockDelta: { contentBlockIndex: i, delta: { toolUse: { input } } } });
const blockStop = (i: number) => ({ contentBlockStop: { contentBlockIndex: i } });

test('text streams as deltas and the summary carries Bedrock usage', async () => {
  const out: any[] = [];
  const s = await relay(from([text('Hel'), text('lo'), stop('end_turn'), usage]), { dsml: false, hasTools: false }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'delta', text: 'Hel' }, { type: 'delta', text: 'lo' }]);
  assert.deepEqual(s, { stopReason: 'end_turn', usage: { inputTokens: 12, outputTokens: 3 }, streamedChars: 5, failed: false });
});

test('reasoning content is never emitted', async () => {
  const out: any[] = [];
  const s = await relay(from([
    { contentBlockDelta: { contentBlockIndex: 0, delta: { reasoningContent: { text: 'thinking...' } } } },
    text('答案', 1), stop('end_turn'), usage]), { dsml: false, hasTools: false }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'delta', text: '答案' }]);
  assert.equal(s.streamedChars, 13);
});

test('a text block with no contentBlockStart streams, and an empty text block emits nothing', async () => {
  const out: any[] = [];
  const s = await relay(from([
    text('', 0), blockStop(0),
    text('Hi', 1), text('', 1), blockStop(1),
    blockStop(2),
    stop('end_turn'), usage]), { dsml: false, hasTools: false }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'delta', text: 'Hi' }]);
  assert.deepEqual(s, { stopReason: 'end_turn', usage: { inputTokens: 12, outputTokens: 3 }, streamedChars: 2, failed: false });
});

test('tool input split across deltas is joined and parsed', async () => {
  const out: any[] = [];
  await relay(from([
    toolStart(1, 't1', 'search_mail'), toolInput(1, '{"query":'), toolInput(1, '"invoice"}'),
    blockStop(1), stop('tool_use'), usage]), { dsml: false, hasTools: true }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'tool_use', toolUseId: 't1', name: 'search_mail', input: { query: 'invoice' } }]);
});

test('empty tool input is {}', async () => {
  const out: any[] = [];
  const s = await relay(from([toolStart(0, 't1', 'list_tasks'), blockStop(0), stop('tool_use'), usage]),
    { dsml: false, hasTools: true }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'tool_use', toolUseId: 't1', name: 'list_tasks', input: {} }]);
  assert.equal(s.failed, false);
});

test('a single tool input chunk is parsed, and tool characters are counted', async () => {
  const out: any[] = [];
  const s = await relay(from([toolStart(0, 't1', 'n'), toolInput(0, '{"a":1}'), blockStop(0), stop('tool_use'), usage]),
    { dsml: false, hasTools: true }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'tool_use', toolUseId: 't1', name: 'n', input: { a: 1 } }]);
  assert.equal(s.streamedChars, 7);
});

test('invalid tool JSON fails the stream and emits no tool_use', async () => {
  const out: any[] = [];
  const s = await relay(from([toolStart(0, 't1', 'n'), toolInput(0, '{"q":'), blockStop(0), stop('tool_use'), usage]),
    { dsml: false, hasTools: true }, (e: any) => out.push(e));
  assert.deepEqual(out, []);
  assert.equal(s.failed, true);
  assert.equal(s.usage, null);
});

test('DeepSeek DSML text is dropped from tool-use turns', async () => {
  const out: any[] = [];
  const s = await relay(from([
    text('<｜DS', 0), text('ML｜function_calls', 0), blockStop(0),
    toolStart(1, 't1', 'search_mail'), toolInput(1, '{"query":"x"}'), blockStop(1),
    stop('tool_use'), usage]), { dsml: true, hasTools: true }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'tool_use', toolUseId: 't1', name: 'search_mail', input: { query: 'x' } }]);
  assert.equal(s.failed, false);
  assert.equal(s.stopReason, 'tool_use');
});

test('DSML mode keeps clean text blocks and original order on tool-use turns', async () => {
  const out: any[] = [];
  await relay(from([
    text('Let me look. ', 0), blockStop(0),
    text('DSML junk', 1), blockStop(1),
    toolStart(2, 't1', 'n'), blockStop(2),
    stop('tool_use'), usage]), { dsml: true, hasTools: true }, (e: any) => out.push(e));
  assert.deepEqual(out, [
    { type: 'delta', text: 'Let me look. ' },
    { type: 'tool_use', toolUseId: 't1', name: 'n', input: {} },
  ]);
});

test('DSML mode without a tool block emits the text once, after messageStop', async () => {
  const seen: string[] = [];
  const out: any[] = [];
  const events = [text('你好'), stop('end_turn'), usage];
  async function* watched() { for (const e of events) { seen.push(Object.keys(e)[0]); yield e; } }
  const s = await relay(watched(), { dsml: true, hasTools: true }, (e: any) => { out.push(e); seen.push('emit'); });
  assert.deepEqual(out, [{ type: 'delta', text: '你好' }]);
  assert.deepEqual(seen, ['contentBlockDelta', 'messageStop', 'emit', 'metadata']);
  assert.equal(s.streamedChars, 2);
});

test('DSML text is kept when dsml is on but the request had no tools', async () => {
  const out: any[] = [];
  await relay(from([text('about DSML files'), stop('end_turn'), usage]), { dsml: true, hasTools: false }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'delta', text: 'about DSML files' }]);
});

test('a stream exception event marks the stream failed and stops reading', async () => {
  const out: any[] = [];
  const s = await relay(from([text('part'), { modelStreamErrorException: { message: 'x' } }, text('more'), stop('end_turn'), usage]),
    { dsml: false, hasTools: false }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'delta', text: 'part' }]);
  assert.deepEqual(s, { stopReason: null, usage: null, streamedChars: 4, failed: true });
});

test('a thrown iterator marks the stream failed', async () => {
  const out: any[] = [];
  const s = await relay(thenThrow([text('part')]), { dsml: false, hasTools: false }, (e: any) => out.push(e));
  assert.deepEqual(out, [{ type: 'delta', text: 'part' }]);
  assert.equal(s.failed, true);
  assert.equal(s.streamedChars, 4);
  assert.equal(s.usage, null);
});

test('a stream with no metadata has null usage', async () => {
  const s = await relay(from([text('a'), stop('end_turn')]), { dsml: false, hasTools: false }, () => {});
  assert.deepEqual(s, { stopReason: 'end_turn', usage: null, streamedChars: 1, failed: false });
});
