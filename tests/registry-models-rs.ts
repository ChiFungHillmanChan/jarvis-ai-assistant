// Reads the model data out of src-tauri/src/ai/models.rs as text, so a test
// can compare the registry with the code the app runs today. It understands
// the shapes that file uses (model(), family(), local(), struct update
// syntax); anything else makes the comparison fail instead of passing quietly.
import { readFileSync } from 'node:fs';

export interface RustPrice { input: number; output: number; cache_read: number; cache_write: number }
export interface RustCloudModel {
  provider: string; id: string; label: string; family: boolean; price: RustPrice;
  long_context: { above: number; price: RustPrice } | null; max_output_tokens: number | null;
}
export interface RustLocalModel {
  id: string; label: string; param_size: string; download_gb: number; min_ram_gb: number;
  tool_capable: boolean; description: string;
  setup: { disk_reserve_gb: number; context_cap: number; think_off: boolean } | null;
}

const num = (text: string) => Number(text.replace(/_/g, ''));

function price(args: string): RustPrice {
  const [input, output, cache_read, cache_write] = args.split(',').map(part => num(part.trim()));
  return { input, output, cache_read, cache_write };
}

/** The text of `name`'s array initialiser, without line comments. */
function arrayBody(source: string, name: string): string {
  const start = source.indexOf(`pub const ${name}:`);
  if (start < 0) throw new Error(`${name} not found in models.rs`);
  const open = source.indexOf('= &[', start) + 4;
  let depth = 1;
  let i = open;
  for (; i < source.length && depth > 0; i++) {
    if (source[i] === '[') depth++;
    if (source[i] === ']') depth--;
  }
  return source.slice(open, i - 1).replace(/\/\/[^\n]*/g, '');
}

/** Split on commas that are not inside (), [] or {}. */
function entries(body: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let current = '';
  for (const ch of body) {
    if ('([{'.includes(ch)) depth++;
    if (')]}'.includes(ch)) depth--;
    if (ch === ',' && depth === 0) { out.push(current.trim()); current = ''; continue; }
    current += ch;
  }
  if (current.trim()) out.push(current.trim());
  return out.filter(Boolean);
}

export function readModelsRs(path: string) {
  const source = readFileSync(path, 'utf8');
  const constants = new Map<string, string>();
  for (const match of source.matchAll(/pub const ([A-Z_]+): &str = "([^"]*)";/g)) constants.set(match[1], match[2]);
  const maxOutput = source.match(/const GEMINI_MAX_OUTPUT: Option<u32> = Some\(([\d_]+)\);/);
  if (!maxOutput) throw new Error('GEMINI_MAX_OUTPUT not found');
  const namedLimits: Record<string, number> = { GEMINI_MAX_OUTPUT: num(maxOutput[1]) };

  const cloud: RustCloudModel[] = entries(arrayBody(source, 'CLOUD_MODELS')).map(entry => {
    const base = entry.match(/\b(model|family)\(\s*"([^"]*)"\s*,\s*"([^"]*)"\s*,\s*(?:"([^"]*)"\s*,\s*)?price\(([^)]*)\)\s*\)/);
    if (!base) throw new Error(`unrecognised CLOUD_MODELS entry: ${entry}`);
    const [, kind, provider, id, label, args] = base;
    const long = entry.match(/long_context:\s*Some\(LongContext\s*\{\s*above:\s*([\d_]+)\s*,\s*price:\s*price\(([^)]*)\)\s*\}\)/);
    const cap = entry.match(/max_output_tokens:\s*([A-Z_]+|Some\(([\d_]+)\)|None)/);
    let max_output_tokens: number | null = null;
    if (cap && cap[1] !== 'None') {
      max_output_tokens = cap[2] ? num(cap[2]) : namedLimits[cap[1]];
      if (max_output_tokens === undefined) throw new Error(`unknown limit ${cap[1]}`);
    }
    if (/long_context:/.test(entry) && !long) throw new Error(`unrecognised long_context in ${entry}`);
    return {
      provider, id, label: kind === 'family' ? id : label, family: kind === 'family', price: price(args),
      long_context: long ? { above: num(long[1]), price: price(long[2]) } : null, max_output_tokens,
    };
  });

  const local: RustLocalModel[] = entries(arrayBody(source, 'LOCAL_MODELS')).map(entry => {
    const base = entry.match(/local\(\s*"([^"]*)"\s*,\s*"([^"]*)"\s*,\s*"([^"]*)"\s*,\s*([\d.]+)\s*,\s*(\d+)\s*,\s*(true|false)\s*,\s*"([^"]*)"\s*\)/);
    if (!base) throw new Error(`unrecognised LOCAL_MODELS entry: ${entry}`);
    const setup = entry.match(/setup:\s*Some\(LocalSetup\s*\{\s*disk_reserve_gb:\s*([\d.]+)\s*,\s*context_cap:\s*([\d_]+)\s*,\s*think_off:\s*(true|false)\s*\}\)/);
    if (/setup:/.test(entry) && !setup) throw new Error(`unrecognised setup in ${entry}`);
    return {
      id: base[1], label: base[2], param_size: base[3], download_gb: num(base[4]), min_ram_gb: num(base[5]),
      tool_capable: base[6] === 'true', description: base[7],
      setup: setup ? { disk_reserve_gb: num(setup[1]), context_cap: num(setup[2]), think_off: setup[3] === 'true' } : null,
    };
  });

  const families = arrayBody(source, 'TOOL_CAPABLE_FAMILIES').match(/"[^"]*"/g)?.map(s => s.slice(1, -1)) ?? [];
  return { constants, cloud, local, toolCapableFamilies: families };
}
