import test from 'node:test';
import assert from 'node:assert/strict';
import { loadTs } from '../../test-support/load-ts.cjs';

const { acquireLease, cachedConfig, usageItem, readGlobalDaily, addGlobalDailyTotal } = loadTs('lambda/ai-proxy/store.ts');

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

/** One in-flight row that evaluates only the two conditional updates acquireLease sends, atomically per call like DynamoDB. */
function inflightTable(row: { n?: number; expiresAt?: number } | undefined) {
  const state = { row: row ? { ...row } : undefined };
  const send = async (c: any) => {
    const { UpdateExpression: update, ConditionExpression: condition, ExpressionAttributeValues: v } = c.input;
    const r = state.row;
    if (update === 'ADD n :one SET expiresAt = :exp' && condition === 'attribute_not_exists(n) OR (n < :max AND expiresAt >= :now)') {
      if (!(r?.n === undefined || (r.n < v[':max'] && r.expiresAt !== undefined && r.expiresAt >= v[':now']))) throw ccf();
      state.row = { n: (r?.n ?? 0) + v[':one'], expiresAt: v[':exp'] };
      return {};
    }
    if (update === 'SET n = :one, expiresAt = :exp' && condition === 'expiresAt < :now') {
      if (!(r?.expiresAt !== undefined && r.expiresAt < v[':now'])) throw ccf();
      state.row = { n: v[':one'], expiresAt: v[':exp'] };
      return {};
    }
    throw new Error(`unexpected command: ${update} / ${condition}`);
  };
  return { state, send };
}

// After a normal release the row is n = 0 with an expiresAt from the last acquire.
const IDLE = { n: 0, expiresAt: 1_800_000_000 - 10 };

test('parallel requests after an idle period all get a slot', async () => {
  const { state, send } = inflightTable(IDLE);
  const granted = await Promise.all(Array.from({ length: 6 }, () => acquireLease(send, 'lim', 'u1', NOW)));
  assert.deepEqual(granted, [true, true, true, true, true, true]);
  assert.equal(state.row!.n, 6);
  assert.equal(state.row!.expiresAt, 1_800_000_360);
});

test('after an idle period a caller past the in-flight max is still refused', async () => {
  const { state, send } = inflightTable(IDLE);
  const granted = await Promise.all(Array.from({ length: 9 }, () => acquireLease(send, 'lim', 'u1', NOW)));
  assert.equal(granted.filter(Boolean).length, 8);
  assert.equal(granted.filter(g => !g).length, 1);
  assert.equal(state.row!.n, 8);
});

test('a lost reset retries the normal acquire once and rethrows its other errors', async () => {
  const sent: any[] = [];
  let n = 0;
  assert.equal(await acquireLease(async (c: any) => { sent.push(c.input); if (n++ < 2) throw ccf(); return {}; }, 'lim', 'u1', NOW), true);
  assert.equal(sent.length, 3);
  assert.deepEqual(sent[2], sent[0]);
  let m = 0;
  await assert.rejects(acquireLease(async () => { if (m++ < 2) throw ccf(); throw new Error('boom3'); }, 'lim', 'u1', NOW), /boom3/);
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
  assert.deepEqual(Object.keys(item).sort(), ['appVersion', 'billedMicros', 'costMicros', 'expiresAt', 'inputTokens', 'latencyMs', 'modelId', 'outputTokens', 'periodKey', 'region', 'requestId', 'sk', 'status', 'ts', 'userId', 'workload']);
  assert.equal(item.sk, '2026-11-15T04:00:00.000Z#r1');
  assert.equal(item.expiresAt, 1794715200 + 400 * 86400);
});

test('the global daily total is one strongly consistent read of the Hong Kong day counter, 0 when absent', async () => {
  const sent: any[] = [];
  const at = new Date('2026-11-15T16:30:00Z'); // 00:30 on 16 Nov in Hong Kong
  assert.equal(await readGlobalDaily(async (c: any) => { sent.push(c); return { Item: { pk: 'day#global#2026-11-16', billedMicros: 4_200 } }; }, 'lim', at), 4_200);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].constructor.name, 'GetCommand');
  assert.deepEqual(sent[0].input, { TableName: 'lim', Key: { pk: 'day#global#2026-11-16' }, ConsistentRead: true });
  assert.equal(await readGlobalDaily(async () => ({}), 'lim', at), 0);
  const before = new Date('2026-11-15T15:59:59Z'); // 23:59:59 on 15 Nov in Hong Kong
  await readGlobalDaily(async (c: any) => { sent.push(c); return {}; }, 'lim', before);
  assert.deepEqual(sent[1].input.Key, { pk: 'day#global#2026-11-15' });
});

test('adding to the global daily total is one atomic ADD on the Hong Kong day counter that expires after two days', async () => {
  const sent: any[] = [];
  const at = new Date('2026-11-15T16:30:00Z');
  const total = await addGlobalDailyTotal(async (c: any) => { sent.push(c); return { Attributes: { billedMicros: 9_818 } }; }, 'lim', 818, at);
  assert.equal(total, 9_818);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].constructor.name, 'UpdateCommand');
  assert.deepEqual(sent[0].input, {
    TableName: 'lim',
    Key: { pk: 'day#global#2026-11-16' },
    UpdateExpression: 'ADD billedMicros :b SET expiresAt = :exp',
    ExpressionAttributeValues: { ':b': 818, ':exp': Math.floor(at.getTime() / 1000) + 2 * 86_400 },
    ReturnValues: 'UPDATED_NEW',
  });
  await assert.rejects(addGlobalDailyTotal(async () => { throw new Error('boom'); }, 'lim', 1, at), /boom/);
});
