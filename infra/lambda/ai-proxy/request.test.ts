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
