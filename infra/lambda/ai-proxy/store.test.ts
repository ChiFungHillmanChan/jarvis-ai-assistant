import test from 'node:test';
import assert from 'node:assert/strict';
import { loadTs } from '../../test-support/load-ts.cjs';

const { acquireLease, cachedConfig, usageItem } = loadTs('lambda/ai-proxy/store.ts');

const ccf = () => Object.assign(new Error('c'), { name: 'ConditionalCheckFailedException' });
const NOW = new Date(1_800_000_000_000);

test('acquire takes a slot when under the limit', async () => {
  const sent: any[] = [];
  assert.equal(await acquireLease(async (c: any) => { sent.push(c.input); return {}; }, 'lim', 'u1', NOW), true);
  assert.equal(sent.length, 1);
  assert.deepEqual(sent[0].ExpressionAttributeValues, { ':one': 1, ':max': 8, ':now': 1_800_000_000, ':exp': 1_800_000_360 });
  assert.deepEqual(sent[0].Key, { pk: 'inflight#u1' });
});

test('acquire resets a stale lease', async () => {
  const sent: any[] = [];
  let n = 0;
  assert.equal(await acquireLease(async (c: any) => { sent.push(c.input); if (n++ === 0) throw ccf(); return {}; }, 'lim', 'u1', NOW), true);
  assert.equal(sent[1].ConditionExpression, 'expiresAt < :now');
  assert.deepEqual(sent[1].ExpressionAttributeValues, { ':one': 1, ':now': 1_800_000_000, ':exp': 1_800_000_360 });
});

test('acquire refuses when both attempts fail and rethrows other errors', async () => {
  assert.equal(await acquireLease(async () => { throw ccf(); }, 'lim', 'u1', NOW), false);
  await assert.rejects(acquireLease(async () => { throw new Error('boom'); }, 'lim', 'u1', NOW), /boom/);
  let n = 0;
  await assert.rejects(acquireLease(async () => { if (n++ === 0) throw ccf(); throw new Error('boom2'); }, 'lim', 'u1', NOW), /boom2/);
});

test('config is cached for 30 seconds', async () => {
  let reads = 0;
  let t = 0;
  const get = cachedConfig(async () => { reads++; return undefined; }, () => t);
  await get();
  assert.equal(reads, 1);
  t = 29_999;
  await get();
  assert.equal(reads, 1);
  t = 30_001;
  await get();
  assert.equal(reads, 2);
});

test('a usage item holds metadata only', () => {
  const item = usageItem({ userId: 'u1', requestId: 'r1', ts: '2026-11-15T04:00:00.000Z', periodKey: 'm2026-11', workload: 'chat', modelId: 'qwen.qwen3-32b-v1:0', region: 'ap-northeast-1', inputTokens: 1, outputTokens: 2, costMicros: 3, billedMicros: 5, status: 'ok', latencyMs: 9, appVersion: null });
  assert.deepEqual(Object.keys(item).sort(), ['billedMicros', 'costMicros', 'expiresAt', 'inputTokens', 'latencyMs', 'modelId', 'outputTokens', 'periodKey', 'region', 'requestId', 'sk', 'status', 'ts', 'userId', 'workload']);
  assert.equal(item.sk, '2026-11-15T04:00:00.000Z#r1');
  assert.equal(item.expiresAt, 1794715200 + 400 * 86400);
});
