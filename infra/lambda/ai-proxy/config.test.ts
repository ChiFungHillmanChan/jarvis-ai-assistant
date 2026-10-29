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

test('every catalog model has a region and a default price', () => {
  for (const id of Object.keys(CATALOG)) { assert.ok(CATALOG[id].region); assert.ok(DEFAULT_CONFIG.prices[id], id); }
});
