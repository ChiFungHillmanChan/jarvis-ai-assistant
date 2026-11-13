// Signs the registry for publishing. Runs only in the Registry workflow's
// sign job, which holds the private key as the REGISTRY_SIGNING_KEY secret
// of the protected `registry-signing` environment:
//
//   node --experimental-strip-types registry/sign.ts registry/registry.json <out-dir>
//
// Writes <out-dir>/registry.json (the canonical bytes) and
// <out-dir>/registry.json.sig (base64 Ed25519 signature over them), after
// checking that the registry is valid, that the key matches the committed
// public key registry/signing-public-key.pem (the key the app will trust),
// and that it is not the app updater's key. Never prints the key.
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { canonicalBytes, rawPublicKey, signBytes, updaterRawPublicKey, verifyBytes } from './signing.ts';
import { validateRegistry } from './validate.ts';

const here = dirname(fileURLToPath(import.meta.url));
export const PUBLIC_KEY_PATH = join(here, 'signing-public-key.pem');
const TAURI_CONF = join(here, '..', 'src-tauri', 'tauri.conf.json');

export interface Signed { bytes: Buffer; signature: string; sha256: string; version: number }

export function signRegistry(doc: unknown, privateKeyPem: string, publicKeyPem: string, updaterPubkey: string | null): Signed {
  const errors = validateRegistry(doc);
  if (errors.length) throw new Error(`the registry is not valid:\n${errors.join('\n')}`);
  if (updaterPubkey && rawPublicKey(publicKeyPem).equals(updaterRawPublicKey(updaterPubkey))) {
    throw new Error('the registry public key is the app updater key; the registry needs its own key');
  }
  const bytes = canonicalBytes(doc);
  const signature = signBytes(bytes, privateKeyPem);
  if (!verifyBytes(bytes, signature, publicKeyPem)) {
    throw new Error('the signing key does not match registry/signing-public-key.pem');
  }
  return { bytes, signature, sha256: createHash('sha256').update(bytes).digest('hex'), version: (doc as { version: number }).version };
}

function main(argv: string[]): number {
  const [input, outDir] = argv;
  if (!input || !outDir) { console.error('usage: sign.ts <registry.json> <out-dir>'); return 2; }
  const privateKeyPem = process.env.REGISTRY_SIGNING_KEY ?? '';
  if (!privateKeyPem.trim()) { console.error('registry: REGISTRY_SIGNING_KEY is not set (the founder adds it to the registry-signing environment)'); return 1; }
  if (!existsSync(PUBLIC_KEY_PATH)) { console.error('registry: registry/signing-public-key.pem is not committed yet (founder action)'); return 1; }
  const updater = JSON.parse(readFileSync(TAURI_CONF, 'utf8'))?.plugins?.updater?.pubkey ?? null;
  try {
    const signed = signRegistry(JSON.parse(readFileSync(input, 'utf8')), privateKeyPem, readFileSync(PUBLIC_KEY_PATH, 'utf8'), updater);
    mkdirSync(outDir, { recursive: true });
    writeFileSync(join(outDir, 'registry.json'), signed.bytes);
    writeFileSync(join(outDir, 'registry.json.sig'), signed.signature);
    console.log(`registry: signed version ${signed.version}, sha256 ${signed.sha256}`);
    return 0;
  } catch (error) {
    console.error(`registry: ${(error as Error).message}`);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) process.exitCode = main(process.argv.slice(2));
