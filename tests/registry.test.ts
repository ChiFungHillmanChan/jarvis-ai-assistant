// The model registry (registry/): the seeded file against the code the app
// runs today, the validator's refusals, and the signing format.
//
// Name every repository file a registry test reads by its full path in a
// string literal ('src-tauri/src/ai/openai.rs', or '../<path>' in a URL): the
// last test checks that the pull-request check runs when any of them changes.
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { generateKeyPairSync } from 'node:crypto';
import { validateRegistry, validateRegistryText, publishedVersionOf } from '../registry/validate.ts';
import { findPriced, resolveModel, costMicros, type RegistryModel } from '../registry/lookup.ts';
import { canonicalJson, canonicalBytes, signBytes, verifyBytes, rawPublicKey, updaterRawPublicKey } from '../registry/signing.ts';
import { signRegistry } from '../registry/sign.ts';
import { readModelsRs } from './registry-models-rs.ts';

const read = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const seeded = () => JSON.parse(read('registry/registry.json'));
const rust = readModelsRs(new URL('../src-tauri/src/ai/models.rs', import.meta.url).pathname);
const constant = (name: string) => rust.constants.get(name);
const model = (doc: any, id: string) => doc.models.find((m: any) => m.id === id);
const provider = (doc: any, id: string) => doc.providers.find((p: any) => p.id === id);
const one = (source: string, pattern: RegExp) => {
  const found = source.match(pattern);
  assert.ok(found, `pattern ${pattern} not found`);
  return found[1];
};

test('the seeded registry is valid', () => {
  assert.deepEqual(validateRegistry(seeded()), []);
});

test('the registry price list is models.rs CLOUD_MODELS, entry for entry and in order', () => {
  const doc = seeded();
  const priced = doc.models.filter((m: any) => !m.managed && m.prices !== null);
  assert.equal(priced.length, rust.cloud.length);
  rust.cloud.forEach((r, i) => {
    const m = priced[i];
    assert.equal(m.id, r.id);
    assert.equal(m.provider, r.provider, r.id);
    assert.equal(m.kind, r.family ? 'family' : 'model', r.id);
    assert.equal(m.display_name.en, r.label, r.id);
    assert.deepEqual(
      { input: m.prices.input, output: m.prices.output, cache_read: m.prices.cache_read, cache_write: m.prices.cache_write },
      r.price, r.id,
    );
    assert.deepEqual(m.prices.tiers, r.long_context
      ? [{ above_input_tokens: r.long_context.above, ...{ input: r.long_context.price.input, output: r.long_context.price.output, cache_read: r.long_context.price.cache_read, cache_write: r.long_context.price.cache_write } }]
      : [], r.id);
    assert.equal(m.max_output, r.max_output_tokens, r.id);
  });
});

test('the registry local catalog is models.rs LOCAL_MODELS and TOOL_CAPABLE_FAMILIES', () => {
  const doc = seeded();
  const local = doc.models.filter((m: any) => m.provider === 'ollama');
  assert.equal(local.length, rust.local.length);
  rust.local.forEach((r, i) => {
    const m = local[i];
    assert.equal(m.id, r.id);
    assert.equal(m.display_name.en, r.label);
    assert.equal(m.prices, null, 'a local model is free');
    assert.equal(m.capabilities.tools, r.tool_capable, r.id);
    assert.deepEqual(m.local, { param_size: r.param_size, download_gb: r.download_gb, min_ram_gb: r.min_ram_gb, description: r.description, setup: r.setup }, r.id);
  });
  assert.deepEqual(provider(doc, 'ollama').tool_capable_families, rust.toolCapableFamilies);
});

test('feature defaults are the models.rs defaults', () => {
  const f = seeded().features;
  for (const feature of ['chat', 'premium', 'mail_analysis', 'mail_sort', 'briefing', 'memory']) {
    assert.equal(f[feature].openai, constant('OPENAI_DEFAULT'), feature);
    assert.equal(f[feature].claude, constant('CLAUDE_DEFAULT'), feature);
    assert.equal(f[feature].bedrock, constant('BEDROCK_DEFAULT'), feature);
    // ai/mod.rs gemini_model_for: Pro only for an explicit premium request.
    assert.equal(f[feature].gemini, feature === 'premium' ? constant('GEMINI_PRO') : constant('GEMINI_FLASH'), feature);
  }
  assert.equal(f.mail_sort.typesafe, constant('JEV'));
  // voice/transcribe.rs sends speech to GEMINI_FLASH; the fallback is local Whisper.
  assert.equal(f.stt.gemini, constant('GEMINI_FLASH'));
  assert.equal(f.stt.whisper, 'ggml-base');
  assert.equal(f.tts.minimax, one(read('src-tauri/src/voice/minimax.rs'), /const MODEL: &str = "([^"]+)";/));
  // Mail settings: V30's column default and the serde default in mail/models.rs.
  assert.equal(f.mail_analysis.openai, one(read('src-tauri/migrations/V30__choose_one_ai.sql'), /openai_model TEXT NOT NULL DEFAULT '([^']+)'/));
  assert.match(read('src-tauri/src/mail/models.rs'), /crate::ai::models::OPENAI_DEFAULT\.into\(\)/);
});

test('the price lookup over the registry gives what models.rs gives', () => {
  const models = seeded().models as RegistryModel[];
  const input = (id: string) => findPriced(models, id)?.prices?.input;
  // The cases of models.rs specific_ids_beat_the_families_they_contain and friends.
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
  assert.equal(findPriced(models, constant('GEMINI_PRO')!)?.max_output, 65_536);
  assert.equal(findPriced(models, 'gemini-flash-latest')?.kind, 'family');
  assert.equal(findPriced(models, 'gemini-flash-latest')?.max_output, null);
  // JARVIS AI's own models match by exact id only, and never through the app's lookup.
  for (const m of models.filter(m => m.managed)) assert.equal(resolveModel(models, m.id, true), m, m.id);
  for (const m of models) assert.ok(!findPriced(models, m.id)?.managed, m.id);
});

test('limits, presets and lists the registry holds match the code', () => {
  const doc = seeded();
  assert.equal(provider(doc, 'openai').limits.chat_max_output_tokens, Number(one(read('src-tauri/src/ai/openai.rs'), /max_completion_tokens: official\.then_some\((\d+)\)/)));
  assert.equal(provider(doc, 'kimi').limits.chat_max_output_tokens, Number(one(read('src-tauri/src/ai/openai.rs'), /max_tokens: \(!official\)\.then_some\((\d+)\)/)));
  assert.equal(provider(doc, 'openai').limits.chat_reasoning_effort, one(read('src-tauri/src/ai/openai.rs'), /reasoning_effort: official\.then_some\("([^"]+)"\)/));
  assert.equal(provider(doc, 'claude').limits.chat_max_output_tokens, Number(one(read('src-tauri/src/ai/claude.rs'), /max_tokens: (\d+),/)));
  assert.equal(provider(doc, 'claude').limits.api_version, one(read('src-tauri/src/ai/claude.rs'), /"anthropic-version", "([^"]+)"/));
  assert.equal(provider(doc, 'gemini').limits.default_max_output_tokens, Number(one(read('src-tauri/src/ai/gemini.rs'), /max_output_tokens\(model\)\.unwrap_or\(([\d_]+)\)/).replace(/_/g, '')));
  assert.equal(provider(doc, 'gemini').limits.stt_max_output_tokens, Number(one(read('src-tauri/src/voice/transcribe.rs'), /"maxOutputTokens": (\d+)/)));
  assert.equal(provider(doc, 'bedrock').limits.chat_max_output_tokens, Number(one(read('src-tauri/src/ai/bedrock.rs'), /\.max_tokens\((\d+)\)\s*\.temperature/)));
  assert.equal(provider(doc, 'bedrock').limits.chat_temperature, Number(one(read('src-tauri/src/ai/bedrock.rs'), /\.temperature\(([\d.]+)\)/)));
  assert.equal(provider(doc, 'ollama').limits.completion_context_tokens, Number(one(read('src-tauri/src/ai/complete.rs'), /const OLLAMA_CONTEXT: u32 = ([\d_]+);/).replace(/_/g, '')));
  assert.equal(provider(doc, 'ollama').limits.default_context_tokens, Number(one(read('src-tauri/src/ai/mod.rs'), /\|row\| row\.get\(0\),\s*\)\s*\.unwrap_or\((\d+)\);/)));
  for (const file of ['src-tauri/src/ai/openai.rs', 'src-tauri/src/ai/claude.rs', 'src-tauri/src/ai/gemini.rs', 'src-tauri/src/ai/bedrock.rs', 'src-tauri/src/ai/local/mod.rs']) {
    assert.equal(Number(one(read(file), /let max_iterations = (\d+);/)), 5, file);
  }
  for (const p of doc.providers) if (p.limits?.max_tool_rounds !== undefined) assert.equal(p.limits.max_tool_rounds, 5, p.id);

  // ai/mod.rs AiRouter::new preset chains.
  const chains = doc.presets.chains;
  assert.equal(doc.presets.default, 'gemini_primary');
  assert.deepEqual(chains.gemini_primary.entries, [{ provider: 'gemini', model: null }, { provider: 'gemini', model: constant('GEMINI_PRO') }]);
  assert.deepEqual(chains.gemini_flash.entries, [{ provider: 'gemini', model: constant('GEMINI_FLASH') }, { provider: 'gemini', model: constant('GEMINI_PRO') }]);
  assert.deepEqual(chains.gemini_pro_only.entries, [{ provider: 'gemini', model: constant('GEMINI_PRO') }]);
  assert.deepEqual(chains.gemini_flash_only.entries, [{ provider: 'gemini', model: constant('GEMINI_FLASH') }]);
  const settings = read('src/pages/Settings.tsx');
  for (const [name, chain] of Object.entries<any>(chains)) {
    assert.ok(settings.includes(`{ value: "${name}", label: text("${chain.label.en}", "${chain.label['zh-HK']}") }`), name);
  }

  // Bedrock: inference geographies and the picker's built-in list.
  assert.deepEqual(provider(doc, 'bedrock').inference_geographies, JSON.parse(one(read('src-tauri/src/ai/mod.rs'), /const INFERENCE_GEOGRAPHIES: \[&str; 3\] = (\[[^\]]+\]);/)));
  const list = read('src-tauri/src/aws/model_list.rs');
  assert.deepEqual(provider(doc, 'bedrock').built_in_models, [constant('BEDROCK_DEFAULT'), one(list, /const SEEDED_HAIKU: &str = "([^"]+)";/), one(list, /const SEEDED_NOVA_MICRO: &str = "([^"]+)";/)]);

  // Setup and compatible services: commands/ai_choice.rs, ai/compat.rs, src/lib/ai-choice.ts, ChooseAi.tsx.
  assert.deepEqual(doc.chat_model_filter.exclude_words, JSON.parse(one(read('src-tauri/src/commands/ai_choice.rs'), /const NOT_CHAT: \[&str; 14\] = (\[[^\]]+\]);/)));
  const compat = read('src-tauri/src/ai/compat.rs');
  assert.deepEqual(provider(doc, 'kimi').regions.map((r: any) => r.base_url), [one(compat, /KIMI_GLOBAL: &str = "([^"]+)"/), one(compat, /KIMI_CHINA: &str = "([^"]+)"/)]);
  assert.deepEqual(provider(doc, 'qwen').regions.map((r: any) => r.base_url), [one(compat, /QWEN_INTERNATIONAL: &str = "([^"]+)"/), one(compat, /QWEN_CHINA: &str = "([^"]+)"/)]);
  const choice = read('src/lib/ai-choice.ts');
  for (const region of [...provider(doc, 'kimi').regions, ...provider(doc, 'qwen').regions]) {
    assert.ok(choice.includes(`baseUrl: '${region.base_url}', keyPage: '${region.key_page}'`), region.base_url);
  }
  const chooseAi = read('src/components/ChooseAi.tsx');
  const order = [...chooseAi.matchAll(/\['(\w+)', [^\n]*text\('[^']*', '[^']*'\)\],/g)].map(m => m[1]);
  const setupIds = doc.providers.filter((p: any) => p.setup).sort((a: any, b: any) => a.setup.order - b.setup.order).map((p: any) => p.id);
  assert.deepEqual(setupIds, order.map(id => (id === 'other' ? 'openai_compat' : id)));
  for (const p of doc.providers.filter((p: any) => p.setup)) {
    assert.ok(chooseAi.includes(`text('${p.setup.hint.en}', '${p.setup.hint['zh-HK']}')`), p.id);
    if (p.setup.key_page) assert.ok(chooseAi.includes(`'${p.setup.key_page}'`), p.id);
  }
  assert.deepEqual(provider(doc, 'ollama_cloud').model_id_suffixes, [':cloud', '-cloud']);
  assert.match(read('src-tauri/src/ai/mail_policy.rs'), /name\.ends_with\(":cloud"\) \|\| name\.ends_with\("-cloud"\)/);

  // voice/model_manager.rs: the Whisper download.
  const manager = read('src-tauri/src/voice/model_manager.rs');
  assert.deepEqual(model(doc, 'ggml-base').download, {
    url: one(manager, /const MODEL_URL: &str =\s*"([^"]+)";/),
    sha256: one(manager, /const MODEL_SHA256: &str = "([^"]+)";/),
    min_bytes: Number(one(manager, /const MIN_MODEL_BYTES: u64 = ([\d_]+);/).replace(/_/g, '')),
  });
});

test('data terms follow ai/mail_policy.rs', () => {
  const doc = seeded();
  const terms = Object.fromEntries(doc.providers.map((p: any) => [p.id, p.data_terms]));
  // ("claude" | "bedrock", _) | ("openai", None) => Ok; ("gemini", _) => billing check; a local model on this Mac => Ok.
  assert.equal(terms.claude, 'safe');
  assert.equal(terms.bedrock, 'safe');
  assert.equal(terms.openai, 'safe');
  assert.equal(terms.ollama, 'safe');
  assert.equal(terms.gemini, 'needs_billing_confirmation');
  // Any other base URL, Gemini's compatible URL and Ollama cloud models are refused today.
  for (const id of ['kimi', 'qwen', 'gemini_compat', 'openai_compat', 'ollama_cloud']) assert.equal(terms[id], 'unknown', id);
  assert.match(read('src-tauri/src/ai/mail_policy.rs'), /\("claude" \| "bedrock", _\) \| \("openai", None\) => Ok\(\(\)\)/);
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
// no workload uses.
const withFallback = (doc: any): string => {
  doc.managed.fallback ??= doc.models.find((m: any) => m.managed && !Object.values(doc.managed.workload_map).includes(m.id))?.id;
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
  // A default that reaches its entry through the price lookup is checked the same way.
  const viaLookup = errorsFor(doc => { model(doc, 'gemini-3.1-pro').status = 'disabled'; });
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
  assert.deepEqual(errorsFor(doc => { doc.features.chat.bedrock = 'us.anthropic.claude-haiku-4-5-20251001-v1:0'; }), []);
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
  for (const file of ['src-tauri/src/ai/models.rs', 'src/pages/Settings.tsx', 'infra/lambda/registry/index.ts']) assert.ok(named.has(file), `the scan finds ${file}`);
  const uncovered = [...named].filter(file => !covered(file)).sort();
  assert.deepEqual(uncovered, [], `add to the pull_request paths of .github/workflows/registry-check.yml: ${uncovered.join(', ')}`);
  // A listed file that no longer exists (renamed or moved) leaves its new name unchecked.
  for (const glob of patterns.filter(g => !g.includes('*'))) assert.ok(existsSync(new URL(glob, repoRoot)), `${glob} is listed but does not exist`);
});
