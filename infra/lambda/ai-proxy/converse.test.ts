import test from 'node:test';
import assert from 'node:assert/strict';
import { loadTs } from '../../test-support/load-ts.cjs';

const { DEFAULT_CONFIG, CATALOG } = loadTs('lambda/ai-proxy/config.ts');
const { converse, ERROR_STATUS, RATE_PER_MINUTE, MAX_IN_FLIGHT, DAILY_ALERT_MICROS } = loadTs('lambda/ai-proxy/converse.ts');
const { userHash, writeLog, writeDailyAlarmMetric } = loadTs('lambda/ai-proxy/log.ts');

const now = new Date('2026-11-15T04:00:00Z');
const grant = (over = {}) => ({ grantId: 'g1', type: 'trial', creditMicros: 20_000_000, validFrom: '2026-11-01T00:00:00Z', validUntil: '2026-12-31T00:00:00Z', createdAt: '2026-11-01T00:00:00Z', ...over });
const ID = '1b4e28ba-2fa1-4d2b-a5e7-2c0d6f8b9a01';
const body = (over = {}) => JSON.stringify({ workload: 'chat', messages: [{ role: 'user', content: [{ text: 'hi' }] }], requestId: ID, ...over });

const text = (t: string) => ({ contentBlockDelta: { contentBlockIndex: 0, delta: { text: t } } });
const stop = (stopReason: string) => ({ messageStop: { stopReason } });
const usage = (inputTokens: number, outputTokens: number) => ({ metadata: { usage: { inputTokens, outputTokens, totalTokens: inputTokens + outputTokens } } });
async function* from(list: any[]) { for (const e of list) yield e; }
async function* thenFail(list: any[]) { for (const e of list) yield e; yield { modelStreamErrorException: {} }; }
const named = (name: string, message = 'x') => Object.assign(new Error(message), { name });

const BILLED = 818;

function fakeDeps(over: Record<string, any> = {}, limitsOver: Record<string, any> = {}) {
  const calls: any = { opened: [], meter: [], released: 0, logs: [], alarms: [], hit: [], acquire: 0, claim: 0, daily: [] };
  const deps: any = {
    now: () => now,
    loadUser: async () => ({ meta: undefined, grants: [grant()] }),
    loadConfig: async () => DEFAULT_CONFIG,
    periodUsed: async () => 1_000_000,
    limits: {
      hit: async (...args: any[]) => { calls.hit.push(args); return true; },
      acquire: async () => { calls.acquire++; return true; },
      release: async () => { calls.released++; },
      claim: async () => { calls.claim++; return true; },
      addDaily: async (_u: string, billed: number) => { calls.daily.push(billed); return billed; },
      ...limitsOver,
    },
    openStream: async (entry: any, input: any) => {
      calls.opened.push({ entry, input });
      return from([text('答案'), stop('end_turn'), usage(1000, 500)]);
    },
    meter: async (record: any) => { calls.meter.push(record); },
    log: (entry: any) => { calls.logs.push(entry); },
    dailyAlarm: (hash: string) => { calls.alarms.push(hash); },
    ...over,
  };
  return { deps, calls };
}

function fakeSink(over: Record<string, any> = {}) {
  const out: any = { status: 200, body: null, events: [], started: false, ended: false };
  const sink: any = {
    fail: (status: number, b: any) => { out.status = status; out.body = b; },
    start: () => { out.started = true; },
    send: (e: any) => { out.events.push(e); },
    end: () => { out.ended = true; },
    ...over,
  };
  return { out, sink };
}

const caller = { userId: 'u1', appVersion: '0.6.0' };

test('constants match the spec', () => {
  assert.deepEqual(ERROR_STATUS, { unauthenticated: 401, bad_request: 400, not_entitled: 402, cap_reached: 402, trial_credit_used: 402, suspended: 403, duplicate_request: 409, rate_limited: 429, paused: 503, model_unavailable: 502, upstream_error: 502 });
  assert.deepEqual(RATE_PER_MINUTE, { chat: 30, mail_analysis: 30, mail_sort: 150, voice: 30 });
  assert.equal(MAX_IN_FLIGHT, 8);
  assert.equal(DAILY_ALERT_MICROS, 20_000_000);
});

test('a granted user streams and is metered from Bedrock usage', async () => {
  const { deps, calls } = fakeDeps(); const { out, sink } = fakeSink();
  await converse(deps, caller, body(), sink);
  assert.equal(out.status, 200);
  assert.equal(out.started, true);
  assert.equal(out.ended, true);
  assert.deepEqual(out.events[0], { type: 'delta', text: '答案' });
  assert.deepEqual(out.events.at(-1), { type: 'done', stopReason: 'end_turn', modelId: 'qwen.qwen3-32b-v1:0', inputTokens: 1000, outputTokens: 500, billedMicros: BILLED, period: { key: 'm2026-11', usedMicros: 1_000_818, capMicros: 20_000_000 } });
  const { ts, latencyMs, ...rec } = calls.meter[0];
  assert.deepEqual(rec, { userId: 'u1', requestId: ID, periodKey: 'm2026-11', workload: 'chat', modelId: 'qwen.qwen3-32b-v1:0', region: 'ap-northeast-1', inputTokens: 1000, outputTokens: 500, costMicros: 545, billedMicros: BILLED, status: 'ok', appVersion: '0.6.0' });
  assert.equal(ts, now.toISOString());
  assert.equal(calls.released, 1);
  assert.equal(calls.logs.length, 1);
  assert.deepEqual({ ...calls.logs[0], latencyMs: 0 }, { userHash: userHash('u1'), workload: 'chat', modelId: 'qwen.qwen3-32b-v1:0', status: 'ok', code: null, inputTokens: 1000, outputTokens: 500, billedMicros: BILLED, latencyMs: 0, meterFailed: false });
});

test('denials return their status and never call Bedrock', async () => {
  const cases: [Record<string, any>, number, Record<string, unknown>][] = [
    [{ loadUser: async () => ({ meta: undefined, grants: [] }) }, 402, { error: 'not_entitled' }],
    [{ loadUser: async () => ({ meta: { aiSuspended: true }, grants: [grant()] }) }, 403, { error: 'suspended' }],
    [{ loadConfig: async () => ({ ...DEFAULT_CONFIG, globalPause: true }) }, 503, { error: 'paused' }],
    [{ periodUsed: async () => 20_000_000 }, 402, { error: 'cap_reached', capMicros: 20_000_000, periodEndsAt: '2026-11-30T16:00:00.000Z' }],
    [{ loadUser: async () => ({ meta: { tier: 'pro', subscriptionStatus: 'trialing' }, grants: [] }), loadConfig: async () => ({ ...DEFAULT_CONFIG, subscriptionAccess: true }), periodUsed: async () => 20_000_000 }, 402, { error: 'trial_credit_used' }],
    // With the shipped config a paid or trialing subscription alone is not access.
    [{ loadUser: async () => ({ meta: { tier: 'pro', subscriptionStatus: 'active' }, grants: [] }) }, 402, { error: 'not_entitled' }],
    [{ loadUser: async () => ({ meta: { tier: 'pro', subscriptionStatus: 'trialing' }, grants: [] }) }, 402, { error: 'not_entitled' }],
  ];
  for (const [over, status, expected] of cases) {
    const { deps, calls } = fakeDeps(over); const { out, sink } = fakeSink();
    await converse(deps, caller, body(), sink);
    assert.equal(out.status, status);
    assert.deepEqual(out.body, expected);
    assert.equal(out.started, false);
    assert.equal(calls.opened.length, 0);
    assert.equal(calls.meter.length, 0);
    assert.equal(calls.hit.length, 0);
    assert.equal(calls.logs.length, 1);
    assert.equal(calls.logs[0].status, 'denied');
    assert.equal(calls.logs[0].code, expected.error);
  }
});

test('bad body is 400 before any limit is touched', async () => {
  const { deps, calls } = fakeDeps(); const { out, sink } = fakeSink();
  await converse(deps, caller, '{not json', sink);
  assert.equal(out.status, 400);
  assert.deepEqual(out.body, { error: 'bad_request' });
  assert.equal(calls.hit.length, 0);
  assert.equal(calls.acquire, 0);
  assert.equal(calls.logs.length, 1);
  assert.equal(calls.logs[0].code, 'bad_request');
});

test('rate limit, concurrency and duplicate requests', async () => {
  {
    const { deps, calls } = fakeDeps({}, { hit: async () => false }); const { out, sink } = fakeSink();
    await converse(deps, caller, body(), sink);
    assert.equal(out.status, 429);
    assert.deepEqual(out.body, { error: 'rate_limited' });
    assert.equal(calls.acquire, 0);
    assert.equal(calls.released, 0);
    assert.equal(calls.opened.length, 0);
  }
  {
    const { deps, calls } = fakeDeps({}, { acquire: async () => false }); const { out, sink } = fakeSink();
    await converse(deps, caller, body(), sink);
    assert.equal(out.status, 429);
    assert.deepEqual(out.body, { error: 'rate_limited' });
    assert.equal(calls.released, 0, 'a slot that was never acquired is not released');
    assert.equal(calls.opened.length, 0);
  }
  {
    const { deps, calls } = fakeDeps({}, { claim: async () => false }); const { out, sink } = fakeSink();
    await converse(deps, caller, body(), sink);
    assert.equal(out.status, 409);
    assert.deepEqual(out.body, { error: 'duplicate_request' });
    assert.equal(calls.released, 1);
    assert.equal(calls.opened.length, 0);
    assert.equal(calls.meter.length, 0);
  }
});

test('mail_sort uses its own rate limit', async () => {
  const { deps, calls } = fakeDeps(); const { sink } = fakeSink();
  await converse(deps, caller, body({ workload: 'mail_sort' }), sink);
  assert.deepEqual(calls.hit, [['u1', 'mail_sort', 150, now]]);
  const chat = fakeDeps(); await converse(chat.deps, caller, body(), fakeSink().sink);
  assert.deepEqual(chat.calls.hit, [['u1', 'chat', 30, now]]);
});

test('primary failure before streaming falls back once', async () => {
  const { deps, calls } = fakeDeps(); const { out, sink } = fakeSink();
  let n = 0;
  deps.openStream = async (entry: any, input: any) => {
    calls.opened.push({ entry, input });
    if (++n === 1) throw named('ThrottlingException');
    return from([text('ok'), stop('end_turn'), usage(1000, 500)]);
  };
  await converse(deps, caller, body(), sink);
  assert.equal(calls.opened.length, 2);
  assert.equal(calls.opened[0].input.modelId, 'qwen.qwen3-32b-v1:0');
  assert.equal(calls.opened[1].input.modelId, 'deepseek.v3.2');
  assert.deepEqual(calls.opened[1].entry, CATALOG['deepseek.v3.2']);
  assert.equal(out.events.at(-1).modelId, 'deepseek.v3.2');
  assert.equal(calls.meter[0].modelId, 'deepseek.v3.2');
  assert.equal(calls.meter[0].costMicros, 1850); // 740 input + 1110 output micros
  assert.equal(calls.logs[0].modelId, 'deepseek.v3.2');
  assert.equal(calls.released, 1);
});

test('both models failing map to the right code', async () => {
  const cases: [string, number, string][] = [
    ['ThrottlingException', 429, 'rate_limited'],
    ['ServiceQuotaExceededException', 429, 'rate_limited'],
    ['AccessDeniedException', 502, 'model_unavailable'],
    ['ResourceNotFoundException', 502, 'model_unavailable'],
    ['ModelNotReadyException', 502, 'model_unavailable'],
    ['ServiceUnavailableException', 502, 'model_unavailable'],
    ['ValidationException', 502, 'upstream_error'],
  ];
  for (const [name, status, code] of cases) {
    const { deps, calls } = fakeDeps({ openStream: async () => { throw named(name); } }); const { out, sink } = fakeSink();
    await converse(deps, caller, body(), sink);
    assert.equal(out.status, status, name);
    assert.deepEqual(out.body, { error: code }, name);
    assert.equal(out.started, false);
    assert.equal(calls.released, 1);
    assert.equal(calls.meter.length, 0);
    assert.equal(calls.logs.length, 1);
    assert.equal(calls.logs[0].code, code);
    assert.equal(calls.logs[0].status, 'error');
  }
});

test('without a fallback a failing primary is not retried', async () => {
  const config = { ...DEFAULT_CONFIG, models: { ...DEFAULT_CONFIG.models, chat: { primary: 'qwen.qwen3-32b-v1:0', fallback: null } } };
  const { deps, calls } = fakeDeps({ loadConfig: async () => config, openStream: async () => { throw named('AccessDeniedException'); } }); const { out, sink } = fakeSink();
  await converse(deps, caller, body(), sink);
  assert.equal(out.status, 502);
  assert.equal(calls.logs.length, 1);
});

test('Bedrock input carries system, tools and maxTokens', async () => {
  const tools = { tools: [{ toolSpec: { name: 'search_mail', inputSchema: { json: { type: 'object' } } } }] };
  const plain = fakeDeps(); await converse(plain.deps, caller, body(), fakeSink().sink);
  assert.equal(plain.calls.opened[0].input.system, undefined);
  assert.equal(plain.calls.opened[0].input.toolConfig, undefined);
  assert.deepEqual(plain.calls.opened[0].input.inferenceConfig, { maxTokens: 2048 });
  assert.deepEqual(plain.calls.opened[0].input.messages, [{ role: 'user', content: [{ text: 'hi' }] }]);

  const full = fakeDeps(); await converse(full.deps, caller, body({ system: 'Be brief', tools, maxTokens: 300 }), fakeSink().sink);
  assert.deepEqual(full.calls.opened[0].input.system, [{ text: 'Be brief' }]);
  assert.deepEqual(full.calls.opened[0].input.toolConfig, tools);
  assert.deepEqual(full.calls.opened[0].input.inferenceConfig, { maxTokens: 300 });
});

test('tool use is relayed and DSML is dropped only for the DSML model with tools', async () => {
  const tools = { tools: [{ toolSpec: { name: 'search_mail', inputSchema: { json: {} } } }] };
  const events = [
    { contentBlockDelta: { contentBlockIndex: 0, delta: { text: 'DSML junk' } } },
    { contentBlockStart: { contentBlockIndex: 1, start: { toolUse: { toolUseId: 't1', name: 'search_mail' } } } },
    { contentBlockDelta: { contentBlockIndex: 1, delta: { toolUse: { input: '{"q":"x"}' } } } },
    { contentBlockStop: { contentBlockIndex: 1 } },
    stop('tool_use'), usage(10, 5),
  ];
  const config = { ...DEFAULT_CONFIG, models: { ...DEFAULT_CONFIG.models, chat: { primary: 'deepseek.v3.2', fallback: null } } };
  const { deps } = fakeDeps({ loadConfig: async () => config, openStream: async () => from(events) }); const { out, sink } = fakeSink();
  await converse(deps, caller, body({ tools }), sink);
  assert.deepEqual(out.events.slice(0, -1), [{ type: 'tool_use', toolUseId: 't1', name: 'search_mail', input: { q: 'x' } }]);
  assert.equal(out.events.at(-1).stopReason, 'tool_use');
});

test('a mid-stream failure meters an estimate and sends an error event', async () => {
  const { deps, calls } = fakeDeps({ openStream: async () => thenFail([text('partial')]) }); const { out, sink } = fakeSink();
  await converse(deps, caller, body(), sink);
  assert.deepEqual(out.events.at(-1), { type: 'error', code: 'upstream_error' });
  assert.equal(out.events.some((e: any) => e.type === 'done'), false);
  assert.equal(out.ended, true);
  assert.equal(calls.meter.length, 1);
  assert.equal(calls.meter[0].status, 'error');
  assert.equal(calls.meter[0].outputTokens, 4);
  assert.equal(calls.meter[0].inputTokens, Math.ceil(body().length / 2));
  assert.equal(calls.released, 1);
  assert.equal(calls.logs.length, 1);
  assert.deepEqual([calls.logs[0].status, calls.logs[0].code], ['error', 'upstream_error']);
});

test('a stream that fails before any output is not metered', async () => {
  const { deps, calls } = fakeDeps(); const { out, sink } = fakeSink();
  deps.openStream = async () => thenFail([]);
  await converse(deps, caller, body(), sink);
  assert.deepEqual(out.events, [{ type: 'error', code: 'upstream_error' }]);
  assert.equal(calls.meter.length, 0);
});

test('client disconnect still meters reported usage and releases', async () => {
  const { deps, calls } = fakeDeps(); const { out, sink } = fakeSink({ send: () => { throw new Error('socket closed'); }, end: () => { throw new Error('socket closed'); } });
  await converse(deps, caller, body(), sink);
  assert.equal(calls.meter.length, 1);
  assert.equal(calls.meter[0].inputTokens, 1000);
  assert.equal(calls.meter[0].outputTokens, 500);
  assert.equal(calls.meter[0].status, 'ok');
  assert.equal(calls.released, 1);
  assert.equal(calls.logs.length, 1);
  assert.equal(calls.logs[0].status, 'ok');
  assert.equal(out.status, 200);
});

test('output without reported usage is metered from an estimate', async () => {
  const { deps, calls } = fakeDeps({ openStream: async () => from([text('abcdefg'), stop('end_turn')]) }); const { out, sink } = fakeSink();
  await converse(deps, caller, body(), sink);
  assert.equal(calls.meter[0].outputTokens, 4);
  assert.equal(calls.meter[0].inputTokens, Math.ceil(body().length / 2));
  assert.equal(out.events.at(-1).type, 'done');
});

test('the daily alarm fires only when this request crosses US$20', async () => {
  const crossing = fakeDeps({}, { addDaily: async () => 20_000_500 });
  await converse(crossing.deps, caller, body(), fakeSink().sink);
  assert.deepEqual(crossing.calls.alarms, [userHash('u1')]);
  const already = fakeDeps({}, { addDaily: async () => 30_000_000 });
  await converse(already.deps, caller, body(), fakeSink().sink);
  assert.deepEqual(already.calls.alarms, []);
  const below = fakeDeps({}, { addDaily: async () => 5_000_000 });
  await converse(below.deps, caller, body(), fakeSink().sink);
  assert.deepEqual(below.calls.alarms, []);
});

test('a meter failure is retried once and flagged, never thrown', async () => {
  let attempts = 0;
  const { deps, calls } = fakeDeps({ meter: async () => { attempts++; throw new Error('ddb down'); } }); const { out, sink } = fakeSink();
  await converse(deps, caller, body(), sink);
  assert.equal(attempts, 2);
  assert.equal(out.events.at(-1).type, 'done');
  assert.equal(calls.logs[0].meterFailed, true);
  assert.equal(calls.logs[0].status, 'ok');
  assert.deepEqual(calls.daily, [BILLED]);

  let once = 0; const retry = fakeDeps({ meter: async () => { if (++once === 1) throw new Error('blip'); } });
  await converse(retry.deps, caller, body(), fakeSink().sink);
  assert.equal(once, 2);
  assert.equal(retry.calls.logs[0].meterFailed, false);
});

test('a failing daily counter does not break the response', async () => {
  const { deps, calls } = fakeDeps({}, { addDaily: async () => { throw new Error('ddb down'); } }); const { out, sink } = fakeSink();
  await converse(deps, caller, body(), sink);
  assert.equal(out.events.at(-1).type, 'done');
  assert.equal(calls.logs.length, 1);
  assert.equal(calls.logs[0].status, 'ok');
});

test('an unexpected throw becomes upstream_error before or after the stream starts', async () => {
  const early = fakeDeps({ loadUser: async () => { throw new Error('boom'); } }); const e = fakeSink();
  await converse(early.deps, caller, body(), e.sink);
  assert.equal(e.out.status, 502);
  assert.deepEqual(e.out.body, { error: 'upstream_error' });
  assert.equal(early.calls.released, 0);
  assert.equal(early.calls.logs.length, 1);

  const late = fakeDeps(); const l = fakeSink();
  late.deps.openStream = async () => (async function* () { yield text('a'); throw new Error('boom'); })();
  await converse(late.deps, caller, body(), l.sink);
  assert.deepEqual(l.out.events.at(-1), { type: 'error', code: 'upstream_error' });
  assert.equal(late.calls.released, 1);
});

test('logs never contain content, even from thrown errors', async (t) => {
  const spies = (['log', 'warn', 'error', 'info', 'debug'] as const).map(m => t.mock.method(console, m, () => {}));
  const consoleText = () => JSON.stringify(spies.flatMap(s => s.mock.calls.map(c => c.arguments)));

  const content = body({ system: 'SECRET-SYSTEM', messages: [{ role: 'user', content: [{ text: 'SECRET-PROMPT' }] }] });
  const runs: [string, Record<string, any>][] = [
    ['stream', { openStream: async () => from([text('SECRET-REPLY'), stop('end_turn'), usage(10, 5)]) }],
    ['loadUser', { loadUser: async () => { throw new Error('SECRET-ERROR'); } }],
    ['openStream', { openStream: async () => { throw named('ValidationException', 'SECRET-ECHO'); } }],
    ['midstream', { openStream: async () => thenFail([text('SECRET-REPLY')]) }],
    ['meter', { meter: async () => { throw new Error('SECRET-METER'); } }],
  ];
  for (const [label, over] of runs) {
    const { deps, calls } = fakeDeps(over); const { sink } = fakeSink();
    await converse(deps, caller, content, sink);
    assert.equal(calls.logs.length, 1, label);
    assert.ok(!JSON.stringify(calls.logs).includes('SECRET'), label);
  }
  assert.ok(!consoleText().includes('SECRET'));
});

test('the log module emits only closed shapes', (t) => {
  const log = t.mock.method(console, 'log', () => {});
  assert.match(userHash('u1'), /^[0-9a-f]{16}$/);
  assert.notEqual(userHash('u1'), userHash('u2'));

  const entry = { userHash: userHash('u1'), workload: 'chat', modelId: 'm', status: 'ok', code: null, inputTokens: 1, outputTokens: 2, billedMicros: 3, latencyMs: 4, meterFailed: false };
  writeLog(entry);
  assert.deepEqual(JSON.parse(log.mock.calls[0].arguments[0]), { event: 'converse', ...entry });

  writeDailyAlarmMetric('abc123', 'prod');
  const emf = JSON.parse(log.mock.calls[1].arguments[0]);
  assert.deepEqual(emf._aws.CloudWatchMetrics, [{ Namespace: 'Jarvis/AiProxy', Dimensions: [['Env']], Metrics: [{ Name: 'UserDailyBilledOver20', Unit: 'Count' }] }]);
  assert.equal(typeof emf._aws.Timestamp, 'number');
  assert.equal(emf.Env, 'prod');
  assert.equal(emf.UserDailyBilledOver20, 1);
  assert.equal(emf.userHash, 'abc123');
});
