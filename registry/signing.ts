// Canonical bytes and Ed25519 signatures for the published registry.
//
// The canonical form is JSON with every object's keys sorted (by UTF-16 code
// unit, as Array.prototype.sort orders them), no whitespace, and numbers and
// strings written as JSON.stringify writes them. For the values a registry
// holds (strings, finite numbers, booleans, null) this is the RFC 8785 (JCS)
// form. The published registry.json IS these bytes, and registry.json.sig is
// the base64 Ed25519 signature over exactly them. A reader verifies the bytes
// it received before parsing them and never re-serialises to verify.
import { createPrivateKey, createPublicKey, sign, verify, type KeyObject } from 'node:crypto';

export function canonicalJson(value: unknown): string {
  if (value === null || typeof value === 'boolean' || typeof value === 'string') return JSON.stringify(value);
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) throw new Error('canonical JSON: numbers must be finite');
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>);
    if (entries.some(([, v]) => v === undefined)) throw new Error('canonical JSON: undefined is not a JSON value');
    entries.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonicalJson(v)}`).join(',')}}`;
  }
  throw new Error(`canonical JSON: unsupported ${typeof value}`);
}

export function canonicalBytes(value: unknown): Buffer {
  return Buffer.from(canonicalJson(value), 'utf8');
}

function ed25519(key: KeyObject, what: string): KeyObject {
  if (key.asymmetricKeyType !== 'ed25519') throw new Error(`${what} is not an Ed25519 key`);
  return key;
}

/** Base64 of the 64-byte Ed25519 signature over `bytes`. */
export function signBytes(bytes: Uint8Array, privateKeyPem: string): string {
  const key = ed25519(createPrivateKey(privateKeyPem), 'the signing key');
  return sign(null, bytes, key).toString('base64');
}

export function verifyBytes(bytes: Uint8Array, signatureBase64: string, publicKeyPem: string): boolean {
  const key = ed25519(createPublicKey(publicKeyPem), 'the public key');
  const signature = Buffer.from(signatureBase64.trim(), 'base64');
  if (signature.length !== 64) return false;
  return verify(null, bytes, key, signature);
}

/** The raw 32-byte public key, for comparing keys. */
export function rawPublicKey(publicKeyPem: string): Buffer {
  const der = ed25519(createPublicKey(publicKeyPem), 'the public key').export({ type: 'spki', format: 'der' });
  return Buffer.from(der.subarray(der.length - 32));
}

/**
 * The raw 32-byte key inside the Tauri updater's minisign public key
 * (tauri.conf.json plugins.updater.pubkey): base64 of a comment line and a
 * base64 line holding "Ed", an 8-byte key id and the 32-byte key.
 */
export function updaterRawPublicKey(minisignPubkeyBase64: string): Buffer {
  const lines = Buffer.from(minisignPubkeyBase64, 'base64').toString('utf8').split('\n').filter(Boolean);
  const raw = Buffer.from(lines[1] ?? '', 'base64');
  if (raw.length !== 42 || raw.subarray(0, 2).toString('latin1') !== 'Ed') throw new Error('not a minisign Ed25519 public key');
  return Buffer.from(raw.subarray(10));
}
