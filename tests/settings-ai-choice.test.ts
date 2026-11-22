import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { MAIN_PATH_MAX_INPUT_USD, cheapestFor, jarvisAiAvailable, mainPathModels, needsChainConfirm } from '../src/settings/ai-choice.ts';
import { PROVIDER_GUIDES, guideFor } from '../src/settings/ai/providerGuides.ts';
import { AWS_REGIONS } from '../src/lib/aws-regions.ts';
import type { AiModelCatalog, CloudModel, SetupChoice } from '../src/lib/ai-models.ts';
import type { ProviderChainEntry } from '../src/lib/types.ts';

type Status = CloudModel['status'];
const model = (provider: string, id: string, input: number, output = input * 4, status: Status = 'active', family = false): CloudModel => ({
  provider, id, label: id, family, status, max_output_tokens: null,
  price: { input_per_million: input, output_per_million: output, cache_read_per_million: 0, cache_write_per_million: 0 },
});
const cloud: CloudModel[] = [
  model('gemini', 'gemini-4-hydrogen', 1.5),
  model('gemini', 'gemini-3.8-flash', 0.75),
  model('gemini', 'gemini-3.1-flash-lite', 0.25),
  model('claude', 'claude-sonnet-5-5', 2),
  model('claude', 'claude-opus-5-5', 4),
  model('claude', 'claude-fable-5-2', 8),
  model('claude', 'claude-haiku-5-5', 0.1),
  model('openai', 'gpt-6-luna', 0.1),
  model('openai', 'gpt-6-astra', 10),
];

test('the main path keeps cheap Gemini models, cheapest first, and drops the expensive ones', () => {
  assert.equal(MAIN_PATH_MAX_INPUT_USD, 1.5);
  assert.deepEqual(mainPathModels(cloud, 'gemini').map(m => m.id), ['gemini-3.1-flash-lite', 'gemini-3.8-flash', 'gemini-4-hydrogen']);
  assert.deepEqual(mainPathModels(cloud, 'claude').map(m => m.id), ['claude-haiku-5-5']);
  assert.deepEqual(mainPathModels(cloud, 'openai').map(m => m.id), ['gpt-6-luna']);
  assert.deepEqual(mainPathModels(cloud, 'nobody'), []);
});

test('cheapestFor picks the lowest-priced active model of the provider', () => {
  assert.equal(cheapestFor(cloud, 'openai')?.id, 'gpt-6-luna');
  assert.equal(cheapestFor(cloud, 'claude')?.id, 'claude-haiku-5-5');
  assert.equal(cheapestFor(cloud, 'gemini')?.id, 'gemini-3.1-flash-lite');
  assert.equal(cheapestFor(cloud, 'kimi'), null);
});

test('a deprecated or disabled model is never offered, however cheap', () => {
  const withOld = [model('gemini', 'gemini-old', 0.05, 0.2, 'deprecated'), model('gemini', 'gemini-gone', 0.01, 0.1, 'disabled'), ...cloud];
  assert.deepEqual(mainPathModels(withOld, 'gemini').map(m => m.id), ['gemini-3.1-flash-lite', 'gemini-3.8-flash', 'gemini-4-hydrogen']);
  assert.equal(cheapestFor(withOld, 'gemini')?.id, 'gemini-3.1-flash-lite');
  assert.equal(cheapestFor([model('kimi', 'k-old', 0.1, 0.4, 'deprecated')], 'kimi'), null);
});

test('a family price pattern is not a model a user can pick', () => {
  const withFamily = [model('gemini', 'gemini-flash', 0.1, 0.4, 'active', true), ...cloud];
  assert.equal(cheapestFor(withFamily, 'gemini')?.id, 'gemini-3.1-flash-lite');
});

test('equal input prices are broken by output price, then by id', () => {
  const tied = [model('qwen', 'b', 0.3, 1), model('qwen', 'a', 0.3, 1), model('qwen', 'c', 0.3, 0.5), model('qwen', 'd', 0.2, 9)];
  assert.deepEqual(mainPathModels(tied, 'qwen').map(m => m.id), ['d', 'c', 'a', 'b']);
  assert.equal(cheapestFor(tied, 'qwen')?.id, 'd');
  assert.equal(cheapestFor(tied.slice(0, 3), 'qwen')?.id, 'c');
  assert.equal(cheapestFor([tied[0], tied[1]], 'qwen')?.id, 'a');
});

test('the provider order is only asked to confirm when there is more than one entry', () => {
  const entry = (position: number): ProviderChainEntry => ({ position, provider_type: 'gemini' } as ProviderChainEntry);
  assert.equal(needsChainConfirm([]), false);
  assert.equal(needsChainConfirm([entry(0)]), false);
  assert.equal(needsChainConfirm([entry(0), entry(1)]), true);
});

test('JARVIS AI is available only when the registry offers it in setup', () => {
  const choice = (id: string): SetupChoice => ({ id, name: { en: id, 'zh-HK': id }, hint: { en: '', 'zh-HK': '' }, key_page: null, model: 'optional' });
  const catalog = (setup: SetupChoice[]) => ({ setup }) as AiModelCatalog;
  assert.equal(jarvisAiAvailable(catalog([choice('openai')])), false);
  assert.equal(jarvisAiAvailable(catalog([choice('openai'), choice('jarvis_ai')])), true);
});

test('the Gemini guide follows the spec: 4 steps, billing warned plainly', () => {
  const gemini = guideFor('gemini', []);
  assert.equal(gemini.steps.length, 4);
  assert.equal(gemini.steps[0].link, 'https://aistudio.google.com/apikey');
  assert.equal(gemini.steps[2].link, 'https://ai.google.dev/gemini-api/docs/billing');
  assert.match(gemini.steps[2].en, /chat only/i);
  assert.match(gemini.steps[2].en, /never for mail or calendar/i);
  assert.match(gemini.steps[3].en, /Test and save/);
});

test('there is one guide per provider, and every step has both languages', () => {
  assert.deepEqual(PROVIDER_GUIDES.map(g => g.id), ['gemini', 'openai', 'claude', 'kimi', 'qwen']);
  for (const guide of PROVIDER_GUIDES) {
    assert.ok(guide.steps.length >= 3, guide.id);
    for (const step of guide.steps) {
      assert.ok(step.en.length > 0 && step.zh.length > 0, guide.id);
      if (step.link) assert.match(step.link, /^https:\/\//);
    }
    assert.match(guide.steps[0].link ?? '', /^https:\/\//, `${guide.id} opens with the key page`);
  }
});

test('the registry key page wins over the fallback link', () => {
  const setup: SetupChoice[] = [{ id: 'openai', name: { en: 'OpenAI', 'zh-HK': 'OpenAI' }, hint: { en: '', 'zh-HK': '' }, key_page: 'https://example.com/registry-keys', model: 'optional' }];
  assert.equal(guideFor('openai', setup).steps[0].link, 'https://example.com/registry-keys');
  assert.notEqual(PROVIDER_GUIDES.find(g => g.id === 'openai')?.steps[0].link, 'https://example.com/registry-keys');
  assert.equal(guideFor('claude', setup).steps[0].link, PROVIDER_GUIDES.find(g => g.id === 'claude')?.steps[0].link);
  assert.equal(guideFor('gemini', setup).steps[2].link, 'https://ai.google.dev/gemini-api/docs/billing');
});

test('a null registry key page keeps the fallback', () => {
  const setup: SetupChoice[] = [{ id: 'kimi', name: { en: 'Kimi', 'zh-HK': 'Kimi' }, hint: { en: '', 'zh-HK': '' }, key_page: null, model: 'required' }];
  assert.match(guideFor('kimi', setup).steps[0].link ?? '', /moonshot/);
});

test('the Chinese guide text is written Chinese, never colloquial Cantonese', () => {
  for (const guide of PROVIDER_GUIDES) for (const step of guide.steps) assert.doesNotMatch(step.zh, /[嘅咗唔係冇啲喺嚟]/, `${guide.id}: ${step.zh}`);
});

test('onboarding offers four AWS regions, London included, from one list', () => {
  assert.equal(AWS_REGIONS.length, 4);
  assert.ok(AWS_REGIONS.some(r => r.id === 'eu-west-2'));
  const onboarding = readFileSync(new URL('../src/pages/OnboardingWizard.tsx', import.meta.url), 'utf8');
  assert.match(onboarding, /import \{ AWS_REGIONS \} from ['"]\.\.\/lib\/aws-regions['"]/);
  assert.doesNotMatch(onboarding, /label: 'eu-west-2/);
});
