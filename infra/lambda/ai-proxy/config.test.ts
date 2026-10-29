import test from 'node:test';
import assert from 'node:assert/strict';
import { loadTs } from '../../test-support/load-ts.cjs';

const { CATALOG, DEFAULT_CONFIG, parseConfig, effectiveConfig } = loadTs('lambda/ai-proxy/config.ts');

test('the shipped defaults pass validation unchanged', () => assert.deepEqual(parseConfig(DEFAULT_CONFIG), DEFAULT_CONFIG));

test('parseConfig rejects unsafe values', () => {
  const bad = [
    { models: { ...DEFAULT_CONFIG.models, chat: { primary: 'amazon.nova-micro-v1:0', fallback: null } } }, // not in catalog
    { models: { ...DEFAULT_CONFIG.models, chat: { primary: 'deepseek.v3.2', fallback: 'deepseek.v3.2' } } },
    { multiplierBps: 9999 }, { includedMicros: 1.5 }, { defaultCapMicros: -1 }, { globalPause: 'no' },
    { prices: { 'qwen.qwen3-32b-v1:0': DEFAULT_CONFIG.prices['qwen.qwen3-32b-v1:0'] } }, // referenced deepseek has no price
    { pricesCheckedOn: '29/10/2026' },
    { models: { ...DEFAULT_CONFIG.models, chat: { primary: 'constructor', fallback: null } } }, // inherited key, not a catalog model
    { models: { ...DEFAULT_CONFIG.models, chat: { primary: 'qwen.qwen3-32b-v1:0', fallback: 'toString' } } },
    { prices: JSON.parse('{"__proto__":{"inputMicrosPerMTok":1,"outputMicrosPerMTok":1}}') },
  ];
  for (const over of bad) assert.equal(parseConfig({ ...DEFAULT_CONFIG, ...over }), null, JSON.stringify(over));
  const { voice, ...missing } = DEFAULT_CONFIG.models;
  assert.equal(parseConfig({ ...DEFAULT_CONFIG, models: missing }), null);
});

test('a __proto__ key in prices cannot alter the parsed prices', () => {
  const prices = JSON.parse('{"__proto__":{"inputMicrosPerMTok":1,"outputMicrosPerMTok":1}}');
  Object.assign(prices, DEFAULT_CONFIG.prices);
  const parsed = parseConfig({ ...DEFAULT_CONFIG, prices });
  assert.deepEqual(parsed, DEFAULT_CONFIG);
  assert.equal(Object.getPrototypeOf(parsed.prices), Object.prototype);
  assert.equal(Object.hasOwn(parsed.prices, '__proto__'), false);
});

test('a missing row uses defaults; an invalid row pauses', () => {
  assert.deepEqual(effectiveConfig(undefined), { config: DEFAULT_CONFIG, invalid: false });
  const broken = effectiveConfig({ config: 'current', body: '{' });
  assert.equal(broken.invalid, true); assert.equal(broken.config.globalPause, true);
  assert.equal(effectiveConfig({ config: 'current', body: JSON.stringify({ ...DEFAULT_CONFIG, globalPause: true }) }).config.globalPause, true);
});

test('subscriptionAccess ships off, is false when a stored row omits it, and must be a boolean', () => {
  assert.equal(DEFAULT_CONFIG.subscriptionAccess, false);
  const { subscriptionAccess, ...missing } = DEFAULT_CONFIG;
  assert.equal(parseConfig(missing).subscriptionAccess, false);
  assert.deepEqual(effectiveConfig({ config: 'current', body: JSON.stringify(missing) }), { config: DEFAULT_CONFIG, invalid: false });
  assert.equal(parseConfig({ ...DEFAULT_CONFIG, subscriptionAccess: true }).subscriptionAccess, true);
  for (const odd of ['true', 1, 0, null, {}]) {
    assert.equal(parseConfig({ ...DEFAULT_CONFIG, subscriptionAccess: odd }), null, JSON.stringify(odd));
    assert.equal(effectiveConfig({ config: 'current', body: JSON.stringify({ ...DEFAULT_CONFIG, subscriptionAccess: odd }) }).invalid, true);
  }
});

test('globalDailyCapMicros ships at US$10, takes the default when a stored row omits it, and must be a positive safe integer', () => {
  assert.equal(DEFAULT_CONFIG.globalDailyCapMicros, 10_000_000);
  const { globalDailyCapMicros, ...missing } = DEFAULT_CONFIG;
  assert.equal(parseConfig(missing).globalDailyCapMicros, DEFAULT_CONFIG.globalDailyCapMicros);
  assert.deepEqual(effectiveConfig({ config: 'current', body: JSON.stringify(missing) }), { config: DEFAULT_CONFIG, invalid: false });
  assert.equal(parseConfig({ ...DEFAULT_CONFIG, globalDailyCapMicros: 3_000_000 }).globalDailyCapMicros, 3_000_000);
  for (const odd of [0, -1, 1.5, 2 ** 53, '10000000', null, true, {}]) {
    assert.equal(parseConfig({ ...DEFAULT_CONFIG, globalDailyCapMicros: odd }), null, JSON.stringify(odd));
    const row = effectiveConfig({ config: 'current', body: JSON.stringify({ ...DEFAULT_CONFIG, globalDailyCapMicros: odd }) });
    assert.equal(row.invalid, true, JSON.stringify(odd));
    assert.equal(row.config.globalPause, true, JSON.stringify(odd));
  }
});

test('every catalog model has a region and a default price', () => {
  for (const id of Object.keys(CATALOG)) { assert.ok(CATALOG[id].region); assert.ok(DEFAULT_CONFIG.prices[id], id); }
});
