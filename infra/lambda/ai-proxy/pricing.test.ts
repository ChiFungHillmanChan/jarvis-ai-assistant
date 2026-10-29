import test from 'node:test';
import assert from 'node:assert/strict';
import { loadTs } from '../../test-support/load-ts.cjs';

const { costMicros, billedMicros, estimateUsage } = loadTs('lambda/ai-proxy/pricing.ts');

const qwen = { inputMicrosPerMTok: 180000, outputMicrosPerMTok: 730000 };
const deepseek = { inputMicrosPerMTok: 740000, outputMicrosPerMTok: 2220000 };

test('cost and billed are integer micros rounded up', () => {
  assert.equal(costMicros({ inputTokens: 1000, outputTokens: 500 }, qwen), 545);
  assert.equal(billedMicros(545, 15000), 818);
  assert.equal(costMicros({ inputTokens: 1, outputTokens: 0 }, qwen), 1);
  assert.equal(billedMicros(1, 15000), 2);
  assert.equal(billedMicros(0, 15000), 0);
  const big = costMicros({ inputTokens: 200000, outputTokens: 8192 }, deepseek);
  assert.equal(big, 166187); assert.ok(Number.isSafeInteger(big));
});

test('estimates round up from characters', () => assert.deepEqual(estimateUsage(1001, 7), { inputTokens: 501, outputTokens: 4 }));

test('cap overshoot is bounded: 8 in flight at the largest request stays under US$1.50', () => {
  // 131,072 input tokens = the largest context in the catalog today; revisit if a bigger-context model is added
  assert.ok(8 * billedMicros(costMicros({ inputTokens: 131072, outputTokens: 8192 }, deepseek), 15000) < 1_500_000);
});
