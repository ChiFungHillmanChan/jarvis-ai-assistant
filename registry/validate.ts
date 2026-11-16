// Validates registry/registry.json: the JSON Schema (registry/schema.json),
// then the rules a schema cannot express. Run in CI before signing and again
// against the published copy before upload:
//
//   node --experimental-strip-types registry/validate.ts registry/registry.json [--published <published registry.json>] [--signed]
//
// The edited file must be exactly JSON.stringify(registry, null, 2) plus a
// newline; with --signed (publish.sh, on the signed bytes) it must be exactly
// the canonical bytes. Either way a key given twice cannot pass.
// Exit code 0 when valid; otherwise every problem is printed and the exit code is 1.
import { readFileSync, realpathSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, join } from 'node:path';
import { Ajv2020, type ErrorObject } from 'ajv/dist/2020.js';
import { resolveModel, type RegistryModel } from './lookup.ts';
import { canonicalJson } from './signing.ts';

const here = dirname(fileURLToPath(import.meta.url));
export const SCHEMA_PATH = join(here, 'schema.json');

const ajv = new Ajv2020({ allErrors: true, strict: true });
const checkSchema = ajv.compile(JSON.parse(readFileSync(SCHEMA_PATH, 'utf8')));

interface Provider {
  id: string;
  wire: string;
  data_terms: string;
  consent_copy_keys: unknown;
  setup: { order: number } | null;
  model_id_pattern: string | null;
  default_for: string[];
}

interface Registry {
  version: number;
  providers: Provider[];
  models: RegistryModel[];
  features: Record<string, Record<string, string>>;
  presets: { default: string; chains: Record<string, { entries: { provider: string; model: string | null }[] }> };
  managed: {
    workload_map: Record<string, string>;
    fallback: string | null;
    bedrock_region: string;
    budget_alert_percent: number[];
    budget_kill_percent: number;
    default_output_tokens: number;
    max_output_tokens: number;
  };
}

/** Strings that look like a secret or a person's data. The registry is public. */
const FORBIDDEN: [RegExp, string][] = [
  [/-----BEGIN [A-Z ]*PRIVATE KEY-----/, 'a private key'],
  [/\bAKIA[0-9A-Z]{16}\b/, 'an AWS access key id'],
  [/\bsk-[A-Za-z0-9_-]{16,}/, 'an API key (sk-...)'],
  [/\bAIza[0-9A-Za-z_-]{30,}/, 'a Google API key'],
  [/\bgh[pousr]_[A-Za-z0-9]{20,}/, 'a GitHub token'],
  [/\bxox[abprs]-[A-Za-z0-9-]{10,}/, 'a Slack token'],
  [/\b(?:whsec|rk_live|sk_live|rk_test|sk_test)_[A-Za-z0-9]{8,}/, 'a Stripe secret'],
  [/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/, 'an email address'],
];

function strings(value: unknown, path: string, out: [string, string][]): [string, string][] {
  if (typeof value === 'string') out.push([path, value]);
  else if (Array.isArray(value)) value.forEach((v, i) => strings(v, `${path}[${i}]`, out));
  else if (value && typeof value === 'object') for (const [k, v] of Object.entries(value)) { strings(k, `${path}.${k} (key)`, out); strings(v, `${path}.${k}`, out); }
  return out;
}

function semanticErrors(r: Registry, publishedVersion: number | undefined): string[] {
  const errors: string[] = [];
  const providers = new Map<string, Provider>();
  for (const p of r.providers) {
    if (providers.has(p.id)) errors.push(`duplicate provider id "${p.id}"`);
    providers.set(p.id, p);
    if ((p.data_terms === 'safe') !== (p.consent_copy_keys === null)) {
      errors.push(`provider "${p.id}": consent_copy_keys must be null exactly when data_terms is safe`);
    }
    if (p.model_id_pattern !== null) {
      try { new RegExp(p.model_id_pattern); } catch { errors.push(`provider "${p.id}": model_id_pattern is not a valid regular expression`); }
    }
  }
  const orders = r.providers.flatMap(p => (p.setup ? [p.setup.order] : []));
  if (new Set(orders).size !== orders.length) errors.push('two providers share a setup order');

  const ids = new Set<string>();
  for (const m of r.models) {
    if (ids.has(m.id)) errors.push(`duplicate model id "${m.id}"`);
    ids.add(m.id);
    const provider = providers.get(m.provider);
    if (!provider) { errors.push(`model "${m.id}": unknown provider "${m.provider}"`); continue; }
    if (m.managed !== (provider.wire === 'jarvis_proxy')) errors.push(`model "${m.id}": managed must be true exactly for JARVIS AI's provider`);
    if (m.kind === 'family' && m.prices === null) errors.push(`model "${m.id}": a family is a price pattern and needs prices`);
    if ('local' in m && provider.wire !== 'ollama') errors.push(`model "${m.id}": local settings on a provider that is not Ollama`);
    if ('bedrock' in m && !m.managed) errors.push(`model "${m.id}": bedrock settings on a model that is not managed`);
    if (m.status === 'active' && m.replacement !== null) errors.push(`model "${m.id}": an active model has no replacement`);
    // JARVIS AI's prices are AWS's; each says where and when it was read.
    if (m.managed && m.prices && !(typeof m.price_source === 'string' && /https:\/\/\S+/.test(m.price_source) && /\b\d{4}-\d{2}-\d{2}\b/.test(m.price_source))) {
      errors.push(`model "${m.id}": a managed price must cite its source in price_source, a URL and the date it was read`);
    }
    if (m.prices) {
      const above = m.prices.tiers.map(t => t.above_input_tokens);
      if (above.some((v, i) => i > 0 && v <= above[i - 1])) errors.push(`model "${m.id}": price tiers must rise`);
    }
  }
  // A configured id (a default, a preset entry, a replacement) must name a
  // model of its own provider. An exact match to a family is refused: a family
  // is a price pattern, not a model one can pick (an id that merely contains a
  // family's id, such as jev-latest, still resolves to it). Bedrock is the
  // exception to the provider rule: its ids are other vendors' models reached
  // through AWS, priced under those vendors' entries.
  const pickable = (where: string, providerId: string | undefined, id: string, model: RegistryModel) => {
    if (model.id === id && model.kind === 'family') errors.push(`${where}: "${id}" is a family, a price pattern, not a model one can pick`);
    if (providerId !== undefined && providerId !== 'bedrock' && model.provider !== providerId) {
      errors.push(`${where}: model "${id}" belongs to provider "${model.provider}", not "${providerId}"`);
    }
  };

  for (const m of r.models) {
    if (m.replacement === null) continue;
    const target = resolveModel(r.models, m.replacement, m.managed);
    if (!target) { errors.push(`model "${m.id}": replacement "${m.replacement}" does not exist`); continue; }
    pickable(`model "${m.id}": replacement`, m.provider, m.replacement, target);
    if (target.status !== 'active') errors.push(`model "${m.id}": replacement "${m.replacement}" is ${target.status}`);
  }

  const usable = (where: string, provider: Provider | undefined, modelId: string) => {
    const managed = provider?.wire === 'jarvis_proxy';
    const model = resolveModel(r.models, modelId, managed);
    if (!model) { errors.push(`${where}: model "${modelId}" does not exist`); return model; }
    pickable(where, provider?.id, modelId, model);
    if (model.status !== 'active') errors.push(`${where}: model "${modelId}" is ${model.status}; a default must be active`);
    return model;
  };

  for (const [feature, defaults] of Object.entries(r.features)) {
    for (const [providerId, modelId] of Object.entries(defaults)) {
      const provider = providers.get(providerId);
      if (!provider) { errors.push(`features.${feature}: unknown provider "${providerId}"`); continue; }
      if (!provider.default_for.includes(feature)) errors.push(`features.${feature}.${providerId}: provider "${providerId}" does not list ${feature} in default_for`);
      usable(`features.${feature}.${providerId}`, provider, modelId);
    }
  }
  for (const p of r.providers) {
    for (const feature of p.default_for) {
      if (!r.features[feature]?.[p.id]) errors.push(`features.${feature}: missing the default for provider "${p.id}"`);
    }
  }

  if (!r.presets.chains[r.presets.default]) errors.push(`presets.default "${r.presets.default}" is not a chain`);
  for (const [name, chain] of Object.entries(r.presets.chains)) {
    chain.entries.forEach((entry, i) => {
      const provider = providers.get(entry.provider);
      if (!provider) errors.push(`presets.chains.${name}[${i}]: unknown provider "${entry.provider}"`);
      if (entry.model !== null) usable(`presets.chains.${name}[${i}]`, provider, entry.model);
    });
  }

  const m = r.managed;
  const managedModel = (where: string, id: string) => {
    const model = resolveModel(r.models, id, true);
    if (!model) { errors.push(`${where}: managed model "${id}" does not exist`); return; }
    if (model.status !== 'active') errors.push(`${where}: managed model "${id}" is ${model.status}`);
    if (!model.prices) errors.push(`${where}: managed model "${id}" has no price`);
    if (!model.bedrock) errors.push(`${where}: managed model "${id}" has no bedrock settings`);
    else if (model.bedrock.region !== m.bedrock_region) errors.push(`${where}: managed model "${id}" is in ${model.bedrock.region}, not ${m.bedrock_region}`);
  };
  for (const [workload, id] of Object.entries(m.workload_map)) managedModel(`managed.workload_map.${workload}`, id);
  if (m.fallback !== null) {
    managedModel('managed.fallback', m.fallback);
    if (Object.values(m.workload_map).includes(m.fallback)) errors.push('managed.fallback must differ from every primary model');
  }
  for (const p of r.providers.filter(p => p.wire === 'jarvis_proxy')) {
    for (const feature of p.default_for) {
      if (!m.workload_map[feature]) errors.push(`managed.workload_map: missing ${feature}, which provider "${p.id}" serves`);
    }
  }
  const alerts = m.budget_alert_percent;
  if (alerts.some((v, i) => i > 0 && v <= alerts[i - 1])) errors.push('managed.budget_alert_percent must rise');
  if (alerts.some(v => v >= m.budget_kill_percent)) errors.push('managed.budget_kill_percent must be above every alert');
  if (m.default_output_tokens > m.max_output_tokens) errors.push('managed.default_output_tokens is above managed.max_output_tokens');

  for (const [path, text] of strings(r, '$', [])) {
    for (const [pattern, what] of FORBIDDEN) {
      if (pattern.test(text)) errors.push(`${path}: looks like ${what}; the registry is public and must never hold keys or user data`);
    }
  }

  if (publishedVersion !== undefined && !(r.version > publishedVersion)) {
    errors.push(`version ${r.version} must be greater than the published version ${publishedVersion}`);
  }
  return errors;
}

/** Every problem with `doc`; empty when it may be published. */
export function validateRegistry(doc: unknown, options: { publishedVersion?: number } = {}): string[] {
  if (!checkSchema(doc)) {
    return (checkSchema.errors ?? []).map((e: ErrorObject) => `schema: ${e.instancePath || '/'} ${e.message}${e.params && 'additionalProperty' in e.params ? ` (${e.params.additionalProperty})` : ''}`);
  }
  return semanticErrors(doc as Registry, options.publishedVersion);
}

/**
 * Every problem with a registry file's text: its exact format, then
 * validateRegistry. JSON.parse keeps the last of two equal keys, so a file
 * whose text is not exactly its own re-serialisation could be signed with
 * other values than a reviewer reads in the diff.
 */
export function validateRegistryText(text: string, options: { publishedVersion?: number; signed?: boolean } = {}): string[] {
  let doc: unknown;
  try { doc = JSON.parse(text); } catch (error) { return [`not JSON: ${(error as Error).message}`]; }
  const expected = options.signed ? canonicalJson(doc) : `${JSON.stringify(doc, null, 2)}\n`;
  const format = text === expected ? [] : [options.signed
    ? 'the signed file is not exactly the canonical bytes of the registry'
    : 'the file is not exactly JSON.stringify(registry, null, 2) plus a newline: a key is given twice or the format differs; reformat it and check that no value changed'];
  return [...format, ...validateRegistry(doc, options)];
}

/** The version of a published registry file. */
export function publishedVersionOf(text: string): number {
  const version = (JSON.parse(text) as { version?: unknown }).version;
  if (!Number.isSafeInteger(version)) throw new Error('the published registry has no integer version');
  return version as number;
}

function main(argv: string[]): number {
  const file = argv.find(a => !a.startsWith('--')) ?? join(here, 'registry.json');
  const published = argv.includes('--published') ? argv[argv.indexOf('--published') + 1] : undefined;
  const text = readFileSync(file, 'utf8');
  const errors = validateRegistryText(text, {
    publishedVersion: published ? publishedVersionOf(readFileSync(published, 'utf8')) : undefined,
    signed: argv.includes('--signed'),
  });
  if (errors.length) {
    for (const e of errors) console.error(`registry: ${e}`);
    return 1;
  }
  console.log(`registry: version ${JSON.parse(text).version} is valid${published ? ' and newer than the published one' : ''}`);
  return 0;
}

// The command line runs only when this file is the entry point, never on
// import (the tests import it). Node loads the entry point by its real path,
// so argv[1] is compared by its real path too: a path through a symlink, such
// as macOS's /tmp or /var, still runs it instead of exiting 0 in silence.
const isMain = (() => {
  try { return import.meta.url === pathToFileURL(realpathSync(process.argv[1] ?? '')).href; } catch { return false; }
})();
if (isMain) process.exitCode = main(process.argv.slice(2));
