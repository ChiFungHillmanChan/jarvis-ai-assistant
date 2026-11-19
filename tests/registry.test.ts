// The model registry (registry/): the seeded file is valid, the shared
// lookup (registry/lookup.ts) prices as the app's does, the validator's
// refusals, and the signing format. The app reads the registry itself since
// P05, so nothing here compares the file with the app's code; the app's own
// tests (src-tauri/src/ai/registry) cover how it reads it.
//
// Name every repository file a registry test reads by its full path in a
// string literal ('src-tauri/tauri.conf.json', or '../<path>' in a URL): the
// last test checks that the pull-request check runs when any of them changes.
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { generateKeyPairSync } from 'node:crypto';
import { validateRegistry, validateRegistryText, publishedVersionOf } from '../registry/validate.ts';
import { findPriced, resolveModel, costMicros, type RegistryModel } from '../registry/lookup.ts';
import { canonicalJson, canonicalBytes, signBytes, verifyBytes, rawPublicKey, updaterRawPublicKey } from '../registry/signing.ts';
import { signRegistry } from '../registry/sign.ts';

const read = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const seeded = () => JSON.parse(read('registry/registry.json'));
// registry/registry.json as seeded (version 1), frozen: the app's Rust tests
// run on the same copy, so the lookup cases below are theirs and editing a
// price in registry/registry.json never changes them.
const fixture = () => JSON.parse(read('src-tauri/src/ai/registry/fixture.json'));
const model = (doc: any, id: string) => doc.models.find((m: any) => m.id === id);
const provider = (doc: any, id: string) => doc.providers.find((p: any) => p.id === id);

test('the seeded registry is valid', () => {
  assert.deepEqual(validateRegistry(seeded()), []);
});

test('the price lookup gives what the app lookup gives, on the frozen fixture', () => {
  const models = fixture().models as RegistryModel[];
  const input = (id: string) => findPriced(models, id)?.prices?.input;
  // The cases of ai/models.rs specific_ids_beat_the_families_they_contain and friends.
  assert.equal(input('gpt-5.6-luna'), 0.20);
  assert.equal(input('gpt-5'), 1.25);
  assert.equal(input('gemini-3.5-flash-lite'), 0.30);
  assert.equal(input('gemini-3.5-flash'), 1.50);
  assert.equal(input('gemini-3.6-flash'), 1.50);
  assert.equal(input('gemini-3.1-flash-lite'), 0.25);
  assert.equal(input('global.anthropic.claude-sonnet-4-6'), 3.0);
  assert.equal(input('amazon.nova-micro-v1:0'), 0.035, 'a BYOK Bedrock id never takes the managed Tokyo price');
  assert.equal(input('apac.amazon.nova-micro-v1:0'), 0.035, 'the managed entry with this exact id is not in the app price list');
  assert.equal(input('jev-1.13.0'), 0.042);
  assert.equal(input('claude-haiku-5-5'), 0.10);
  assert.equal(input('gpt-7-mystery'), undefined, 'an unknown model has no price');
  assert.equal(input('qwen3.5:4b'), undefined, 'a local model is not in the price list');
  const cost = (id: string, i: number, o: number, cr: number) => costMicros(findPriced(models, id)!, i, o, cr, 0);
  assert.equal(cost('claude-sonnet-4-6', 1000, 500, 0), 10_500);
  assert.equal(cost('claude-sonnet-4-6', 1000, 0, 800), 840);
  assert.equal(cost('gpt-6-luna', 200_000, 100_000, 0), 70_000);
  assert.equal(cost('gpt-6-luna', 300_000, 0, 0), 60_000);
  assert.equal(cost('gpt-6-luna', 250_000, 0, 0), 25_000);
  assert.equal(cost('gemini-3.5-flash', 250_000, 0, 0), 375_000);
  assert.equal(cost('claude-haiku-5-5', 50_000, 10_000, 0), 10_000);
  assert.equal(cost('claude-haiku-5-5', 200_000, 0, 0), 100_000);
  assert.ok(cost('gemini-3.1-pro-preview', 250_000, 1_000, 0)! > cost('gemini-3.1-pro-preview', 100_000, 1_000, 0)! * 2);
  // label_for and max_output_tokens.
  assert.equal(findPriced(models, 'global.anthropic.claude-sonnet-4-6')?.display_name.en, 'Claude Sonnet 4.6');
  assert.equal(findPriced(models, fixture().features.premium.gemini)?.max_output, 65_536);
  assert.equal(findPriced(models, 'gemini-flash-latest')?.kind, 'family');
  assert.equal(findPriced(models, 'gemini-flash-latest')?.max_output, null);
  // JARVIS AI's own models match by exact id only, and never through the app's lookup.
  for (const m of models.filter(m => m.managed)) assert.equal(resolveModel(models, m.id, true), m, m.id);
  for (const m of models) assert.ok(!findPriced(models, m.id)?.managed, m.id);
});

// ---------------------------------------------------------------------------
// Validator refusals.

const errorsFor = (change: (doc: any) => void, publishedVersion?: number) => {
  const doc = seeded();
  change(doc);
  return validateRegistry(doc, { publishedVersion });
};

// The tests below take JARVIS AI's models from the copy's own settings, never
// from today's values, so that editing the workload map, the fallback, the
// region or the kill level in registry.json never changes what a test means.
// The copy's fallback: the file's own, or, when it has none, a managed entry
// with its own Bedrock settings and prices that no workload uses.
const withFallback = (doc: any): string => {
  doc.managed.fallback ??= doc.models.find((m: any) => m.managed && m.bedrock && m.prices && !Object.values(doc.managed.workload_map).includes(m.id))?.id;
  return doc.managed.fallback;
};

test('the validator rejects a missing feature default', () => {
  assert.ok(errorsFor(doc => { delete doc.features.chat.openai; }).some(e => e.includes('features.chat: missing the default for provider "openai"')));
  assert.ok(errorsFor(doc => { delete doc.features.tts; }).some(e => e.startsWith('schema:') && e.includes('tts')));
  assert.ok(errorsFor(doc => { doc.features.cloud_triage = {}; }).length > 0);
  assert.ok(errorsFor(doc => { delete doc.managed.workload_map.mail_sort; }).some(e => e.includes('missing mail_sort')));
});

test('the validator rejects a disabled or deprecated model used as a default', () => {
  const disabled = errorsFor(doc => { model(doc, 'gpt-6-luna').status = 'disabled'; model(doc, 'gpt-6-luna').replacement = 'gpt-5.6-luna'; });
  assert.ok(disabled.some(e => e.includes('features.chat.openai: model "gpt-6-luna" is disabled')), disabled.join('\n'));
  // A default that reaches its entry through the price lookup is checked the
  // same way. On the frozen fixture, where gemini-3.1-pro-preview reaches the
  // gemini-3.1-pro entry; the editable file names that model exactly.
  const viaLookup = (() => { const doc = fixture(); model(doc, 'gemini-3.1-pro').status = 'disabled'; return validateRegistry(doc); })();
  assert.ok(viaLookup.some(e => e.includes('features.premium.gemini: model "gemini-3.1-pro-preview" is disabled')));
  assert.ok(viaLookup.some(e => e.includes('presets.chains.gemini_pro_only[0]')));
  assert.ok(errorsFor(doc => { model(doc, 'claude-sonnet-5-5').status = 'deprecated'; model(doc, 'claude-sonnet-5-5').replacement = 'claude-opus-5-5'; })
    .some(e => e.includes('is deprecated; a default must be active')));
  assert.ok(errorsFor(doc => { model(doc, doc.managed.workload_map.chat).status = 'disabled'; }).some(e => e.includes('managed.workload_map.chat')));
  assert.ok(errorsFor(doc => { doc.features.chat.openai = 'gpt-9-nonexistent'; }).some(e => e.includes('does not exist')));
});

test('the validator rejects another provider\'s model as a default, a preset entry or a replacement', () => {
  const cross = errorsFor(doc => { doc.features.chat.openai = 'claude-sonnet-5-5'; });
  assert.ok(cross.includes('features.chat.openai: model "claude-sonnet-5-5" belongs to provider "claude", not "openai"'), cross.join('\n'));
  assert.ok(errorsFor(doc => { doc.presets.chains.gemini_flash_only.entries[0].model = 'gpt-6-luna'; })
    .includes('presets.chains.gemini_flash_only[0]: model "gpt-6-luna" belongs to provider "openai", not "gemini"'));
  assert.ok(errorsFor(doc => { model(doc, 'gpt-5.6-luna').status = 'deprecated'; model(doc, 'gpt-5.6-luna').replacement = 'claude-opus-5-5'; })
    .includes('model "gpt-5.6-luna": replacement: model "claude-opus-5-5" belongs to provider "claude", not "openai"'));
  // An id reached through the price lookup is checked the same way.
  assert.ok(errorsFor(doc => { doc.features.chat.gemini = 'gpt-6-luna-x'; }).some(e => e.includes('belongs to provider "openai", not "gemini"')));
  // Bedrock is the exception: its ids are other vendors' models, priced under them.
  // The entry it resolves to is made active on the copy, whatever its status in the file.
  assert.deepEqual(errorsFor(doc => {
    Object.assign(model(doc, 'claude-haiku-4-5'), { status: 'active', replacement: null });
    doc.features.chat.bedrock = 'us.anthropic.claude-haiku-4-5-20251001-v1:0';
  }), []);
});

test('the validator rejects a family, a price pattern, as a default, a preset entry or a replacement', () => {
  const family = errorsFor(doc => { doc.features.chat.gemini = 'gemini-flash'; });
  assert.ok(family.includes('features.chat.gemini: "gemini-flash" is a family, a price pattern, not a model one can pick'), family.join('\n'));
  assert.ok(errorsFor(doc => { doc.presets.chains.gemini_pro_only.entries[0].model = 'gemini-pro'; })
    .includes('presets.chains.gemini_pro_only[0]: "gemini-pro" is a family, a price pattern, not a model one can pick'));
  assert.ok(errorsFor(doc => { model(doc, 'gpt-4o-mini').status = 'deprecated'; model(doc, 'gpt-4o-mini').replacement = 'gpt-5'; })
    .includes('model "gpt-4o-mini": replacement: "gpt-5" is a family, a price pattern, not a model one can pick'));
  // An id that only contains a family's id resolves to that family and is accepted (the mail classifier's jev-latest).
  const doc = seeded();
  doc.features.mail_sort.typesafe = 'jev-latest';
  const models = doc.models as RegistryModel[];
  assert.deepEqual([resolveModel(models, 'jev-latest', false)?.id, resolveModel(models, 'jev-latest', false)?.kind], ['jev', 'family']);
  assert.deepEqual(validateRegistry(doc), []);
});

test('the validator refuses a limit outside its bound, as the app does, and accepts one at it', () => {
  // The bounds of src-tauri/src/ai/registry/doc.rs (Limits::check), mirrored in the schema.
  const cases: [string, string, unknown, unknown][] = [
    ['openai', 'chat_max_output_tokens', 0, 1],
    ['openai', 'chat_max_output_tokens', 131073, 131072],
    ['ollama', 'chat_max_output_tokens', 0, 1],
    ['ollama', 'chat_max_output_tokens', 131073, 131072],
    ['gemini', 'default_max_output_tokens', 131073, 131072],
    ['gemini', 'stt_max_output_tokens', 0, 131072],
    ['ollama', 'completion_context_tokens', 1048577, 1048576],
    ['ollama', 'default_context_tokens', 0, 1],
    ['claude', 'max_tool_rounds', 0, 1],
    ['claude', 'max_tool_rounds', 51, 50],
    ['bedrock', 'chat_temperature', -0.01, 0],
    ['bedrock', 'chat_temperature', 2.01, 2],
    ['openai', 'chat_reasoning_effort', 'maximal', 'max'],
    ['claude', 'api_version', '2023-6-01', '2099-12-31'],
  ];
  for (const [id, field, outside, inside] of cases) {
    const index = seeded().providers.findIndex((p: any) => p.id === id);
    const errors = errorsFor(doc => { provider(doc, id).limits[field] = outside; });
    assert.ok(errors.some(e => e.startsWith(`schema: /providers/${index}/limits/${field} `)), `${id}.${field} = ${outside}: ${errors.join('\n')}`);
    assert.deepEqual(errorsFor(doc => { provider(doc, id).limits[field] = inside; }), [], `${id}.${field} = ${inside}`);
  }
});

test('the validator requires both cache rates on every priced Claude model, in every tier, as the app does', () => {
  // Registry::check in src-tauri/src/ai/registry/doc.rs (lacks_a_claude_cache_rate): Claude
  // requests use Anthropic's prompt cache, and a missing rate prices those tokens at $0.
  const claude = (doc: any) => doc.models.find((m: any) => m.provider === 'claude' && m.prices);
  const refused = (errors: string[]) => errors.some(e => e.endsWith('a Claude model needs both cache_read and cache_write, in every price tier too'));
  const tier = (cacheRead: number | null, cacheWrite: number | null) => ({ above_input_tokens: 400000, input: 9, output: 9, cache_read: cacheRead, cache_write: cacheWrite });
  for (const field of ['cache_read', 'cache_write']) {
    assert.ok(refused(errorsFor(doc => { claude(doc).prices[field] = null; })), field);
  }
  assert.deepEqual(errorsFor(doc => { claude(doc).prices.tiers.push(tier(1, 1)); }), []);
  assert.ok(refused(errorsFor(doc => { claude(doc).prices.tiers.push(tier(null, 1)); })));
  assert.ok(refused(errorsFor(doc => { claude(doc).prices.tiers.push(tier(1, null)); })));
  // A Bedrock row for a Claude model is held to the same rule.
  assert.ok(refused(errorsFor(doc => {
    const row = structuredClone(doc.models.find((m: any) => m.provider === 'bedrock' && !m.managed && m.prices));
    Object.assign(row, { id: 'anthropic.claude-test-v1:0' });
    row.prices.cache_write = null;
    doc.models.push(row);
  })));
  // Another provider's model may leave them out.
  assert.deepEqual(errorsFor(doc => { Object.assign(doc.models.find((m: any) => m.provider === 'gemini' && m.prices).prices, { cache_read: null, cache_write: null }); }), []);
});

test("the validator refuses a feature's output cap outside its bound or a feature it does not know, as the schema says, and accepts a copy without them", () => {
  // FeatureLimits::check in src-tauri/src/ai/registry/doc.rs (MAX_OUTPUT_TOKENS).
  for (const feature of ['mail_analysis', 'mail_sort']) {
    for (const [outside, inside] of [[0, 1], [131073, 131072]]) {
      const errors = errorsFor(doc => { doc.feature_limits[feature].max_output_tokens = outside; });
      assert.ok(errors.some(e => e.startsWith(`schema: /feature_limits/${feature}/max_output_tokens `)), `${feature} = ${outside}: ${errors.join('\n')}`);
      assert.deepEqual(errorsFor(doc => { doc.feature_limits[feature].max_output_tokens = inside; }), [], `${feature} = ${inside}`);
    }
  }
  assert.ok(errorsFor(doc => { doc.feature_limits.not_a_feature = { max_output_tokens: 4096 }; }).some(e => e.startsWith('schema: /feature_limits ')), 'a feature name outside the list');
  assert.ok(errorsFor(doc => { doc.feature_limits.mail_sort.temperature = 1; }).some(e => e.includes('(temperature)')), 'a field the app does not know');
  // A copy written before version 3 (as dev's version 2), or with part of them, is still valid.
  assert.deepEqual(errorsFor(doc => { delete doc.feature_limits; }), []);
  assert.deepEqual(errorsFor(doc => { doc.feature_limits = { mail_analysis: {} }; }), []);
});

test("the validator refuses a model's max_output outside its bound, as the app does, and accepts one at it", () => {
  // MAX_MODEL_OUTPUT_TOKENS in src-tauri/src/ai/registry/doc.rs.
  const index = seeded().models.findIndex((m: any) => m.max_output !== null);
  const set = (value: number) => (doc: any) => { doc.models[index].max_output = value; };
  for (const [outside, inside] of [[0, 1], [1048577, 1048576]]) {
    const errors = errorsFor(set(outside));
    assert.ok(errors.some(e => e.startsWith(`schema: /models/${index}/max_output `)), `${outside}: ${errors.join('\n')}`);
    assert.deepEqual(errorsFor(set(inside)), [], `${inside}`);
  }
});

test('the validator refuses a model id the app could not send safely, such as one with path characters', () => {
  const bad = 'gemini-3.7-flash/../x';
  const refused = (change: (doc: any) => void, path: string) => {
    const errors = errorsFor(change);
    assert.ok(errors.some(e => e.startsWith(`schema: ${path} must match pattern`)), `${path}: ${errors.join('\n')}`);
  };
  refused(doc => { doc.features.chat.gemini = bad; }, '/features/chat/gemini');
  refused(doc => { doc.presets.chains.gemini_flash_only.entries[0].model = 'gemini-3.7-flash?x=1'; }, '/presets/chains/gemini_flash_only/entries/0/model');
  refused(doc => { Object.assign(model(doc, 'gpt-5.6-luna'), { status: 'deprecated', replacement: '../gpt-6-luna' }); }, `/models/${seeded().models.findIndex((m: any) => m.id === 'gpt-5.6-luna')}/replacement`);
  refused(doc => { model(doc, 'gpt-4o').id = 'gpt 4o'; }, `/models/${seeded().models.findIndex((m: any) => m.id === 'gpt-4o')}/id`);
  refused(doc => { provider(doc, 'bedrock').built_in_models[0] = 'us.anthropic.claude#x'; }, `/providers/${seeded().providers.findIndex((p: any) => p.id === 'bedrock')}/built_in_models/0`);
  // Every id in the seeded file is safe, Bedrock profiles and Ollama tags included.
  assert.deepEqual(validateRegistry(seeded()), []);
});

test('the validator rejects a duplicate id', () => {
  assert.ok(errorsFor(doc => { doc.models.push({ ...model(doc, 'gpt-6-luna') }); }).some(e => e === 'duplicate model id "gpt-6-luna"'));
  assert.ok(errorsFor(doc => { doc.providers.push({ ...provider(doc, 'openai'), setup: null }); }).some(e => e === 'duplicate provider id "openai"'));
});

test('the validator rejects a version that is not greater than the published one', () => {
  assert.deepEqual(errorsFor(doc => { doc.version = 1; }, 0), []);
  assert.ok(errorsFor(doc => { doc.version = 1; }, 1).some(e => e === 'version 1 must be greater than the published version 1'));
  assert.ok(errorsFor(doc => { doc.version = 6; }, 7).some(e => e === 'version 6 must be greater than the published version 7'));
  assert.deepEqual(errorsFor(doc => { doc.version = 8; }, 7), []);
  assert.equal(publishedVersionOf(canonicalJson({ version: 7, other: true })), 7);
  assert.throws(() => publishedVersionOf('{"version":"7"}'));
});

test('the validator refuses a file not exactly in its own format, such as one with a key given twice', () => {
  const text = read('registry/registry.json');
  assert.deepEqual(validateRegistryText(text), []);
  const twice = text.replace(/\n    "markup": ([^,\n]+),\n/, '\n    "markup": 0,\n    "markup": $1,\n');
  assert.notEqual(twice, text);
  assert.deepEqual(JSON.parse(twice), JSON.parse(text), 'JSON.parse keeps the last of two equal keys');
  assert.ok(validateRegistryText(twice).some(e => e.startsWith('the file is not exactly JSON.stringify(registry, null, 2)')));
  assert.ok(validateRegistryText(`${JSON.stringify(JSON.parse(text), null, 4)}\n`).some(e => e.startsWith('the file is not exactly')));
  // The signed file is the canonical bytes, and only those.
  assert.deepEqual(validateRegistryText(canonicalJson(seeded()), { signed: true }), []);
  assert.ok(validateRegistryText(text, { signed: true }).includes('the signed file is not exactly the canonical bytes of the registry'));
});

test('the validator rejects anything that looks like a key or user data', () => {
  assert.ok(errorsFor(doc => { provider(doc, 'openai').data_terms_note = 'key sk-proj-abcdefghijklmnopqrstuvwxyz'; }).some(e => e.includes('must never hold keys or user data')));
  assert.ok(errorsFor(doc => { provider(doc, 'openai').data_terms_note = 'AKIAABCDEFGHIJKLMNOP'; }).some(e => e.includes('AWS access key')));
  assert.ok(errorsFor(doc => { provider(doc, 'openai').data_terms_note = 'someone@example.com'; }).some(e => e.includes('email address')));
  assert.ok(errorsFor(doc => { doc.secret = 'x'; }).some(e => e.startsWith('schema:')), 'unknown top-level fields are refused');
});

test('the validator checks the rest of the cross-references', () => {
  assert.ok(errorsFor(doc => { doc.features.chat.nobody = 'gpt-6-luna'; }).some(e => e.includes('unknown provider "nobody"')));
  assert.ok(errorsFor(doc => { doc.features.tts.openai = 'gpt-6-luna'; }).some(e => e.includes('does not list tts in default_for')));
  assert.ok(errorsFor(doc => { provider(doc, 'gemini').consent_copy_keys = null; }).some(e => e.includes('consent_copy_keys')));
  assert.ok(errorsFor(doc => { model(doc, 'gpt-5.6-luna').status = 'deprecated'; model(doc, 'gpt-5.6-luna').replacement = 'gpt-9'; }).some(e => e.includes('replacement "gpt-9" does not exist')));
  assert.ok(errorsFor(doc => { doc.managed.fallback = doc.managed.workload_map.chat; }).some(e => e.includes('fallback must differ')));
  const elsewhere = (region: string) => (region === 'us-east-1' ? 'us-west-2' : 'us-east-1');
  assert.ok(errorsFor(doc => { model(doc, withFallback(doc)).bedrock.region = elsewhere(doc.managed.bedrock_region); })
    .some(e => e.startsWith('managed.fallback:') && e.endsWith(`, not ${seeded().managed.bedrock_region}`)));
  for (const id of seeded().models.filter((m: any) => m.managed && m.prices).map((m: any) => m.id)) {
    assert.ok(errorsFor(doc => { delete model(doc, id).price_source; }).includes(`model "${id}": a managed price must cite its source in price_source, a URL and the date it was read`), id);
  }
  const chat = seeded().managed.workload_map.chat;
  assert.ok(errorsFor(doc => { model(doc, chat).price_source = 'the AWS pricing page'; }).some(e => e.includes(`"${chat}": a managed price must cite its source`)));
  assert.ok(errorsFor(doc => { doc.managed.budget_kill_percent = 150; doc.managed.budget_alert_percent = [50, 80, 160]; }).some(e => e.includes('budget_kill_percent')));
  assert.ok(errorsFor(doc => { doc.presets.default = 'nope'; }).some(e => e.includes('presets.default')));
  assert.ok(errorsFor(doc => { model(doc, 'gpt-4o').prices.tiers = [{ above_input_tokens: 5, input: 1, output: 1, cache_read: 1, cache_write: 1 }, { above_input_tokens: 5, input: 1, output: 1, cache_read: 1, cache_write: 1 }]; }).some(e => e.includes('tiers must rise')));
});

test('the validator refuses a JARVIS AI price of 0, a rate outside the peg band and an unknown time zone', () => {
  const managedPriced = seeded().models.filter((m: any) => m.managed && m.prices).map((m: any) => m.id);
  assert.ok(managedPriced.length > 0);
  for (const id of managedPriced) {
    assert.ok(errorsFor(doc => { model(doc, id).prices.input = 0; }).includes(`model "${id}": every JARVIS AI price must be greater than 0`), id);
  }
  const chat = seeded().managed.workload_map.chat;
  assert.ok(errorsFor(doc => { model(doc, chat).prices.output = 0; }).includes(`model "${chat}": every JARVIS AI price must be greater than 0`));
  assert.ok(errorsFor(doc => { model(doc, chat).prices.cache_read = 0; }).includes(`model "${chat}": every JARVIS AI price must be greater than 0`));

  for (const rate of [7.7, 7.86, 78, 0.78]) {
    assert.ok(errorsFor(doc => { doc.managed.hkd_per_usd = rate; }).includes(`managed.hkd_per_usd ${rate} is outside 7.75 to 7.85, the band of the Hong Kong dollar's peg`), String(rate));
  }
  for (const rate of [7.75, 7.8, 7.85]) {
    assert.ok(!errorsFor(doc => { doc.managed.hkd_per_usd = rate; }).some(e => e.startsWith('managed.hkd_per_usd')), String(rate));
  }

  assert.ok(errorsFor(doc => { doc.managed.daily_cap_time_zone = 'Asia/HongKong'; })
    .includes('managed.daily_cap_time_zone "Asia/HongKong" is not a time zone this runtime knows (for example UTC or Asia/Hong_Kong)'));
  for (const zone of ['UTC', 'Asia/Hong_Kong']) {
    assert.ok(!errorsFor(doc => { doc.managed.daily_cap_time_zone = zone; }).some(e => e.startsWith('managed.daily_cap_time_zone')), zone);
  }
});

// ---------------------------------------------------------------------------
// Signing. A throwaway key pair is made in memory for each test; no key is on disk.

const keyPair = () => {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  return { privatePem: privateKey.export({ type: 'pkcs8', format: 'pem' }) as string, publicPem: publicKey.export({ type: 'spki', format: 'pem' }) as string };
};

test('canonical JSON sorts keys at every level and has no whitespace', () => {
  assert.equal(canonicalJson({ b: [3, { z: 1, a: null }], a: 'xé', c: 2.5, d: true }), '{"a":"xé","b":[3,{"a":null,"z":1}],"c":2.5,"d":true}');
  assert.equal(canonicalJson({ markup: 2.0, rate: 0.10 }), '{"markup":2,"rate":0.1}');
  assert.equal(canonicalBytes({ b: 1, a: 2 }).toString('utf8'), canonicalBytes({ a: 2, b: 1 }).toString('utf8'));
  assert.throws(() => canonicalJson({ a: Number.NaN }));
  assert.throws(() => canonicalJson({ a: undefined }));
  // Non-ASCII stays UTF-8, as the published bytes are verified as received.
  assert.deepEqual([...canonicalBytes({ t: '設定' })].slice(6, 9), [...Buffer.from('設', 'utf8')]);
});

test('a signature verifies over the exact bytes and fails on any change', () => {
  const { privatePem, publicPem } = keyPair();
  const bytes = canonicalBytes(seeded());
  const signature = signBytes(bytes, privatePem);
  assert.equal(Buffer.from(signature, 'base64').length, 64);
  assert.ok(verifyBytes(bytes, signature, publicPem));
  const tampered = Buffer.from(bytes);
  tampered[tampered.length - 2] ^= 1;
  assert.ok(!verifyBytes(tampered, signature, publicPem));
  assert.ok(!verifyBytes(bytes, signature, keyPair().publicPem), 'another key does not verify');
  assert.ok(!verifyBytes(bytes, 'AAAA', publicPem));
});

test('signing refuses an invalid registry, a mismatched public key and the updater key', () => {
  const { privatePem, publicPem } = keyPair();
  const signed = signRegistry(seeded(), privatePem, publicPem, null);
  assert.equal(signed.version, seeded().version);
  assert.equal(signed.bytes.toString('utf8'), canonicalJson(seeded()));
  assert.ok(verifyBytes(signed.bytes, signed.signature, publicPem));

  const invalid = seeded();
  delete invalid.features.chat.openai;
  assert.throws(() => signRegistry(invalid, privatePem, publicPem, null), /not valid/);
  assert.throws(() => signRegistry(seeded(), privatePem, keyPair().publicPem, null), /does not match/);

  // A minisign public key (the Tauri updater format) holding the same raw key.
  const minisign = Buffer.concat([Buffer.from('Ed'), Buffer.alloc(8, 7), rawPublicKey(publicPem)]).toString('base64');
  const updater = Buffer.from(`untrusted comment: minisign public key: 0707070707070707\n${minisign}\n`).toString('base64');
  assert.ok(updaterRawPublicKey(updater).equals(rawPublicKey(publicPem)));
  assert.throws(() => signRegistry(seeded(), privatePem, publicPem, updater), /updater key/);
  // The real updater key in tauri.conf.json parses, and is not this test key.
  const real = JSON.parse(read('src-tauri/tauri.conf.json')).plugins.updater.pubkey;
  assert.equal(updaterRawPublicKey(real).length, 32);
  assert.doesNotThrow(() => signRegistry(seeded(), privatePem, publicPem, real));
});

// ---------------------------------------------------------------------------
// The pull-request check (.github/workflows/registry-check.yml) runs only when
// a pull request touches one of its paths, so a repository file the registry
// tests read but the paths miss could change without the check ever running.

const repoRoot = new URL('..', import.meta.url);
const pullRequestPaths = (workflow: string) => {
  const lines = read(workflow).split('\n');
  const start = lines.indexOf('    paths:', lines.indexOf('  pull_request:'));
  assert.ok(lines.includes('  pull_request:') && start > 0, `${workflow} has a pull_request paths list`);
  const paths: string[] = [];
  for (const line of lines.slice(start + 1)) {
    const item = line.match(/^      - "([^"]+)"$/);
    if (item) paths.push(item[1]);
    else if (!/^      #/.test(line)) break;
  }
  return paths;
};
// GitHub's path filters: ** matches any characters, * any but "/".
const globToRegExp = (glob: string) => new RegExp(`^${glob.split('**')
  .map(part => part.split('*').map(text => text.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('[^/]*'))
  .join('.*')}$`);

test('the pull-request check runs on every repository file the registry tests read', () => {
  const patterns = pullRequestPaths('.github/workflows/registry-check.yml');
  const covered = (file: string) => patterns.some(glob => globToRegExp(glob).test(file));
  const sources = readdirSync(new URL('.', import.meta.url)).filter(name => /^registry.*\.ts$/.test(name)).map(name => `tests/${name}`);
  assert.ok(sources.includes('tests/registry.test.ts') && sources.includes('tests/registry-publish.test.ts'), sources.join(', '));
  // Every string literal in the test sources that names a file of the repository.
  const literal = /'((?:[^'\\\n]|\\.)+)'|"((?:[^"\\\n]|\\.)+)"|`((?:[^`\\$\n]|\\.)+)`/g;
  const named = new Set<string>(sources);
  for (const source of sources) {
    for (const match of read(source).matchAll(literal)) {
      const path = (match[1] ?? match[2] ?? match[3]).replace(/^\.\.\//, '');
      if (path.startsWith('node_modules') || path.startsWith('/')) continue;
      const url = new URL(path, repoRoot);
      if (existsSync(url) && statSync(url).isFile()) named.add(path);
    }
  }
  for (const file of ['src-tauri/src/ai/registry/fixture.json', 'src-tauri/tauri.conf.json', 'infra/lambda/registry/index.ts']) assert.ok(named.has(file), `the scan finds ${file}`);
  const uncovered = [...named].filter(file => !covered(file)).sort();
  assert.deepEqual(uncovered, [], `add to the pull_request paths of .github/workflows/registry-check.yml: ${uncovered.join(', ')}`);
  // A listed file that no longer exists (renamed or moved) leaves its new name unchecked.
  for (const glob of patterns.filter(g => !g.includes('*'))) assert.ok(existsSync(new URL(glob, repoRoot)), `${glob} is listed but does not exist`);
});
