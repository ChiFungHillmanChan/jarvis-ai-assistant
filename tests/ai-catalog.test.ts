import test from 'node:test';
import assert from 'node:assert/strict';
import { cloudModelFor, formatRate, mailSetupModels, registrySourceLabel, serviceFor, serviceLabel, setupOptions } from '../src/lib/ai-catalog.ts';
import type { AiModelCatalog, CloudModel, LocalModel } from '../src/lib/ai-models.ts';

const price = (input: number) => ({ input_per_million: input, output_per_million: input * 4, cache_read_per_million: 0, cache_write_per_million: 0 });
const cloud = (id: string, input: number): CloudModel => ({ provider: 'openai', id, label: id, family: false, price: price(input), max_output_tokens: null });
const local = (id: string, setup: boolean): LocalModel => ({
  id, label: id, param_size: '', download_gb: 3.4, min_ram_gb: 16, tool_capable: true, description: '',
  setup: setup ? { disk_reserve_gb: 10, context_cap: 4096, think_off: true } : null,
});
const catalog: AiModelCatalog = {
  defaults: { openai: 'gpt-6-luna', claude: '', gemini_flash: '', gemini_pro: '', bedrock: '', whisper: '' },
  cloud: [cloud('gpt-5', 1.25), cloud('gpt-6-luna', 0.1), cloud('qwen3-32b', 0.042)],
  local: [local('qwen3.5:4b', true), local('llama3.1:8b', false), local('qwen3.5:9b', true)],
  registry: { version: 7, issued_at: '2026-11-13T00:00:00Z', source: 'cached' },
  registry_updates: true,
  app_version: '0.5.12',
  presets: [{ id: 'a', label: { en: 'A', 'zh-HK': '甲' } }],
  default_preset: 'a',
  setup: [
    { id: 'claude', name: { en: 'Claude', 'zh-HK': 'Claude' }, hint: { en: 'h', 'zh-HK': '提示' }, key_page: 'https://example.com/keys', model: 'optional' },
    { id: 'newcomer', name: { en: 'New', 'zh-HK': '新' }, hint: { en: '', 'zh-HK': '' }, key_page: null, model: 'required' },
    { id: 'openai_compat', name: { en: 'Other service', 'zh-HK': '其他服務' }, hint: { en: '', 'zh-HK': '' }, key_page: null, model: 'required' },
  ],
  compatible: [
    { id: 'kimi', name: 'Kimi', regions: [{ id: 'global', base_url: 'https://api.moonshot.ai/v1', key_page: 'https://example.com/k' }], host_suffixes: ['moonshot.ai', 'moonshot.cn'] },
    { id: 'qwen', name: 'Qwen', regions: [], host_suffixes: ['aliyuncs.com'] },
  ],
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

test('a base URL is named by the registry service whose host it is on, else by its host', () => {
  assert.equal(serviceFor(catalog, 'https://api.moonshot.cn/v1')?.id, 'kimi');
  assert.equal(serviceLabel(catalog, 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1'), 'Qwen');
  assert.equal(serviceLabel(catalog, 'https://API.DeepSeek.com/v1'), 'api.deepseek.com');
  assert.equal(serviceLabel(catalog, 'not a url'), 'not a url');
  assert.equal(serviceFor(null, 'https://api.moonshot.ai/v1'), null);
  // Only at a label boundary, as ai/compat.rs matches.
  assert.equal(serviceFor(catalog, 'https://moonshot.ai/v1')?.id, 'kimi');
  for (const fake of ['https://evilmoonshot.ai/v1', 'https://moonshot.ai.evil.example/v1', 'https://x-aliyuncs.com/v1']) assert.equal(serviceFor(catalog, fake), null, fake);
});

test('setup offers the registry services in its order, only those this build has a form for', () => {
  assert.deepEqual(setupOptions(catalog).map(choice => [choice.id, choice.option]), [['claude', 'claude'], ['openai_compat', 'other']]);
  assert.deepEqual(setupOptions(null), []);
});

test('About names every registry source', () => {
  const en = (english: string) => english;
  const zh = (_: string, chinese: string) => chinese;
  for (const source of ['bundled', 'cached', 'remote'] as const) {
    assert.ok(registrySourceLabel(source, en).length > 0);
    assert.match(registrySourceLabel(source, zh), /[\u4e00-\u9fff]/);
  }
  assert.notEqual(registrySourceLabel('cached', en), registrySourceLabel('remote', en));
});
