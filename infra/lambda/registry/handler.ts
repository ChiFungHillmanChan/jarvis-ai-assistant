// GET /v1/registry: the signed model registry, public and without auth.
//
// The body is the exact bytes of registry/latest.json in the registry bucket,
// which are the canonical JSON the Registry workflow signed; the base64
// Ed25519 signature over them is returned in X-Registry-Signature. The
// signature is stored as object metadata on latest.json itself, so one read
// returns a body and a signature that belong together.
//
// The registry holds no keys and no user data; nothing here reads a request
// body, a header other than If-None-Match, or any identity.
import type { APIGatewayProxyEventV2, APIGatewayProxyStructuredResultV2 } from 'aws-lambda';
import { createHash } from 'node:crypto';

export interface StoredRegistry {
  body: Uint8Array;
  /** Base64 Ed25519 signature from the object's metadata, if present. */
  signature: string | undefined;
}

export type ReadRegistry = () => Promise<StoredRegistry>;

export const CACHE_CONTROL = 'public, max-age=300';
const SIGNATURE = /^[A-Za-z0-9+/]{86}==$/;

const unavailable: APIGatewayProxyStructuredResultV2 = {
  statusCode: 503,
  headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' },
  body: JSON.stringify({ error: 'registry_unavailable' }),
};

/** A strong ETag over the body and its signature. */
export function etagFor(body: Uint8Array, signature: string): string {
  return `"${createHash('sha256').update(body).update('\n').update(signature).digest('hex')}"`;
}

/** RFC 9110 If-None-Match: a list of entity tags or "*", compared weakly. */
export function matchesIfNoneMatch(header: string | undefined, etag: string): boolean {
  if (!header) return false;
  if (header.trim() === '*') return true;
  const opaque = (tag: string) => tag.trim().replace(/^W\//, '');
  return header.split(',').some(tag => opaque(tag) === opaque(etag));
}

export function makeHandler(read: ReadRegistry, log: (message: string) => void = message => console.error(message)) {
  return async function handler(event: APIGatewayProxyEventV2): Promise<APIGatewayProxyStructuredResultV2> {
    let stored: StoredRegistry;
    try {
      stored = await read();
    } catch (error) {
      // The name only (NoSuchKey, AccessDenied before the first publish, a
      // timeout): never a stack trace or a message that could carry a path.
      log(`registry read failed: ${error instanceof Error ? error.name : 'unknown'}`);
      return unavailable;
    }
    const signature = stored.signature?.trim();
    if (!signature || !SIGNATURE.test(signature) || stored.body.length === 0) {
      log('registry read failed: the stored registry has no valid signature');
      return unavailable;
    }
    const etag = etagFor(stored.body, signature);
    const headers = { ETag: etag, 'Cache-Control': CACHE_CONTROL, 'X-Registry-Signature': signature };
    const ifNoneMatch = event.headers?.['if-none-match'] ?? event.headers?.['If-None-Match'];
    if (matchesIfNoneMatch(ifNoneMatch, etag)) return { statusCode: 304, headers };
    return {
      statusCode: 200,
      headers: { ...headers, 'Content-Type': 'application/json' },
      body: Buffer.from(stored.body).toString('base64'),
      isBase64Encoded: true,
    };
  };
}
