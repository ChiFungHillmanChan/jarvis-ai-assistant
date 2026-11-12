import test from 'node:test';
import assert from 'node:assert/strict';
import { loadTs } from '../../test-support/load-ts.cjs';

const { parseRequest } = loadTs('lambda/ai-proxy/request.ts');

const id = '1b4e28ba-2fa1-4d2b-a5e7-2c0d6f8b9a01';
const ok = (over = {}) => JSON.stringify({ workload: 'chat', messages: [{ role: 'user', content: [{ text: 'hi' }] }], requestId: id, ...over });

test('a minimal request gets the defaults', () => {
  const raw = ok();
  assert.deepEqual(parseRequest(raw), { workload: 'chat', system: null, messages: [{ role: 'user', content: [{ text: 'hi' }] }], tools: null, maxTokens: 2048, requestId: id, requestChars: raw.length });
});

test('requestChars counts UTF-8 bytes, so Chinese text is not undercounted', () => {
  const raw = ok({ messages: [{ role: 'user', content: [{ text: '你好' }] }] });
  assert.equal(parseRequest(raw).requestChars, Buffer.byteLength(raw));
  assert.ok(parseRequest(raw).requestChars > raw.length);
});

test('maxTokens is clamped; nonsense falls back to the default', () => {
  assert.equal(parseRequest(ok({ maxTokens: 99999 })).maxTokens, 8192);
  assert.equal(parseRequest(ok({ maxTokens: 300 })).maxTokens, 300);
  for (const m of [0, -5, 2.5, 'x']) assert.equal(parseRequest(ok({ maxTokens: m })).maxTokens, 2048);
});

test('the client tool loop is allowed: toolUse and toolResult blocks', () => {
  const messages = [
    { role: 'user', content: [{ text: 'find invoices' }] },
    { role: 'assistant', content: [{ toolUse: { toolUseId: 't1', name: 'search_mail', input: { query: 'invoice' } } }] },
    { role: 'user', content: [{ toolResult: { toolUseId: 't1', content: [{ json: { count: 2 } }] } }] },
  ];
  assert.ok(parseRequest(ok({ messages, tools: { tools: [{ toolSpec: { name: 'search_mail' } }] } })));
});

test('anything outside the contract is rejected', () => {
  for (const raw of ['{', ok({ workload: 'image' }), ok({ requestId: 'abc' }), ok({ messages: [] }), ok({ system: 7 }),
    ok({ messages: [{ role: 'user', content: [{ image: { format: 'png' } }] }] }),
    ok({ messages: [{ role: 'system', content: [{ text: 'x' }] }] }),
    ok({ tools: { toolChoice: {} } }), ok({ system: 'x'.repeat(524288) })])
    assert.equal(parseRequest(raw), null, raw.slice(0, 60));
});

const toolSpecs = (n: number) => ({ tools: Array.from({ length: n }, (_, i) => ({ toolSpec: { name: `tool_${i}` } })) });
const withResult = (content: unknown[]) => [
  { role: 'user', content: [{ text: 'find invoices' }] },
  { role: 'assistant', content: [{ toolUse: { toolUseId: 't1', name: 'search_mail', input: {} } }] },
  { role: 'user', content: [{ toolResult: { toolUseId: 't1', content } }] },
];

test('chat and voice may send tools; mail workloads may not', () => {
  for (const workload of ['chat', 'voice']) assert.ok(parseRequest(ok({ workload, tools: toolSpecs(1) })), workload);
  for (const workload of ['mail_sort', 'mail_analysis']) {
    assert.equal(parseRequest(ok({ workload, tools: toolSpecs(1) })), null, workload);
    assert.ok(parseRequest(ok({ workload })), `${workload} without tools`);
  }
});

test('at most 16 tools per request', () => {
  assert.ok(parseRequest(ok({ tools: toolSpecs(16) })));
  assert.equal(parseRequest(ok({ tools: toolSpecs(17) })), null);
});

test('toolResult content carries only text or JSON', () => {
  assert.ok(parseRequest(ok({ messages: withResult([{ text: 'two found' }, { json: { count: 2 } }]), tools: toolSpecs(1) })));
  const bad: unknown[] = [
    { image: { format: 'png', source: { bytes: 'AAAA' } } },
    { document: { format: 'pdf', name: 'a', source: { s3Location: { uri: 's3://b/k' } } } },
    { video: { format: 'mp4', source: { s3Location: { uri: 's3://b/k' } } } },
    { s3Location: { uri: 's3://b/k' } },
    { text: 'x', json: {} },
    { text: 'x', extra: 1 },
    { text: 7 },
    { json: 'not an object' },
    { json: null },
    {},
    'text',
  ];
  for (const item of bad) assert.equal(parseRequest(ok({ messages: withResult([{ text: 'fine' }, item]), tools: toolSpecs(1) })), null, JSON.stringify(item));
  assert.equal(parseRequest(ok({ messages: [{ role: 'user', content: [{ toolResult: { toolUseId: 't1' } }] }], tools: toolSpecs(1) })), null);
});

test('each tool entry is exactly { toolSpec: {...} }: no cachePoint, system tool or extra key', () => {
  const bad: unknown[] = [
    { cachePoint: { type: 'default' } },
    { systemTool: { name: 'nova_grounding' } },
    { toolSpec: { name: 'a' }, cachePoint: { type: 'default' } },
    { toolSpec: 'a' },
    { toolSpec: null },
    { toolSpec: [] },
    {},
    null,
    'toolSpec',
  ];
  for (const entry of bad) assert.equal(parseRequest(ok({ tools: { tools: [{ toolSpec: { name: 'ok' } }, entry] } })), null, JSON.stringify(entry));
  assert.ok(parseRequest(ok({ tools: { tools: [{ toolSpec: { name: 'ok' } }] } })));
});

test('a toolResult with empty content is rejected, a non-empty one is not', () => {
  assert.equal(parseRequest(ok({ messages: withResult([]), tools: toolSpecs(1) })), null);
  assert.ok(parseRequest(ok({ messages: withResult([{ text: 'x' }]), tools: toolSpecs(1) })));
});

test('toolUse and toolResult blocks need tools in the request', () => {
  assert.equal(parseRequest(ok({ messages: withResult([{ text: 'x' }]) })), null);
  assert.equal(parseRequest(ok({ workload: 'mail_analysis', messages: withResult([{ text: 'x' }]) })), null);
  const useOnly = [{ role: 'assistant', content: [{ toolUse: { toolUseId: 't1', name: 'search_mail', input: {} } }] }];
  assert.equal(parseRequest(ok({ messages: useOnly })), null);
  for (const workload of ['chat', 'voice']) assert.ok(parseRequest(ok({ workload, messages: withResult([{ text: 'x' }]), tools: toolSpecs(1) })), workload);
});
