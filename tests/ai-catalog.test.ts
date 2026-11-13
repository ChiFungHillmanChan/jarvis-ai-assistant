import test from 'node:test';
import assert from 'node:assert/strict';
import { cloudModelFor, formatRate, mailSetupModels } from '../src/lib/ai-catalog.ts';
import type { AiModelCatalog, CloudModel, LocalModel } from '../src/lib/ai-models.ts';

const price = (input: number) => ({ input_per_million: input, output_per_million: input * 4, cache_read_per_million: 0, cache_write_per_million: 0 });
const cloud = (id: string, input: number): CloudModel => ({ provider: 'openai', id, label: id, family: false, price: price(input), max_output_tokens: null });
const local = (id: string, setup: boolean): LocalModel => ({
  id, label: id, param_size: '', download_gb: 3.4, min_ram_gb: 16, tool_capable: true, description: '',
  setup: setup ? { disk_reserve_gb: 10, context_cap: 4096, think_off: true } : null,
});
const catalog: AiModelCatalog = {
  defaults: { openai: 'gpt-6-luna', claude: '', gemini_flash: '', gemini_pro: '', bedrock: '' },
  cloud: [cloud('gpt-5', 1.25), cloud('gpt-6-luna', 0.1), cloud('qwen3-32b', 0.042)],
  local: [local('qwen3.5:4b', true), local('llama3.1:8b', false), local('qwen3.5:9b', true)],
};

test('mail setup offers only the approved local models, in catalog order', () => {
  assert.deepEqual(mailSetupModels(catalog).map(model => model.id), ['qwen3.5:4b', 'qwen3.5:9b']);
  assert.deepEqual(mailSetupModels(null), []);
});

test('a model id is priced by the longest catalog id it contains, as ai/models.rs does', () => {
  assert.equal(cloudModelFor(catalog, 'gpt-6-luna')?.id, 'gpt-6-luna');
  assert.equal(cloudModelFor(catalog, ' GPT-5-mini-2026 ')?.id, 'gpt-5');
  assert.equal(cloudModelFor(catalog, 'some-new-model'), null);
  assert.equal(cloudModelFor(catalog, ''), null);
  assert.equal(cloudModelFor(null, 'gpt-6-luna'), null);
});

test('rates read as prices: two decimals unless the rate has more', () => {
  assert.equal(formatRate(0.1), '0.10');
  assert.equal(formatRate(3), '3.00');
  assert.equal(formatRate(0.042), '0.042');
});
