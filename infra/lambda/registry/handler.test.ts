import test from 'node:test';
import assert from 'node:assert/strict';
import { makeHandler, etagFor, matchesIfNoneMatch, CACHE_CONTROL, type StoredRegistry } from './handler.ts';

const body = new TextEncoder().encode('{"features":{},"version":3}');
const signature = Buffer.alloc(64, 7).toString('base64');
const event = (headers: Record<string, string> = {}) => ({ headers, rawPath: '/v1/registry' }) as any;

function harness(read: () => Promise<StoredRegistry>) {
  const logs: string[] = [];
  return { handler: makeHandler(read, message => logs.push(message)), logs };
}

test('the registry is served as its exact bytes with the signature, an ETag and a five-minute cache', async () => {
  const { handler } = harness(async () => ({ body, signature }));
  const response = await handler(event());
  assert.equal(response.statusCode, 200);
  assert.equal(response.isBase64Encoded, true);
  assert.deepEqual(Buffer.from(response.body!, 'base64'), Buffer.from(body));
  assert.equal(response.headers!['Content-Type'], 'application/json');
  assert.equal(response.headers!['Cache-Control'], CACHE_CONTROL);
  assert.match(String(response.headers!['Cache-Control']), /max-age=300/);
  assert.equal(response.headers!['X-Registry-Signature'], signature);
  assert.match(String(response.headers!.ETag), /^"[0-9a-f]{64}"$/);
  assert.equal(response.headers!.ETag, etagFor(body, signature));
});

test('the ETag changes when the body or the signature changes', () => {
  const other = Buffer.alloc(64, 8).toString('base64');
  assert.notEqual(etagFor(body, signature), etagFor(new TextEncoder().encode('{"version":4}'), signature));
  assert.notEqual(etagFor(body, signature), etagFor(body, other));
});

test('a matching If-None-Match gets 304 with no body', async () => {
  const { handler } = harness(async () => ({ body, signature }));
  const etag = etagFor(body, signature);
  for (const header of [etag, `W/${etag}`, `"stale", ${etag}`, '*']) {
    const response = await handler(event({ 'if-none-match': header }));
    assert.equal(response.statusCode, 304, header);
    assert.equal(response.body, undefined);
    assert.equal(response.headers!.ETag, etag);
    assert.equal(response.headers!['Cache-Control'], CACHE_CONTROL);
  }
  const stale = await handler(event({ 'if-none-match': '"stale"' }));
  assert.equal(stale.statusCode, 200);
  assert.equal(matchesIfNoneMatch(undefined, etag), false);
  assert.equal(matchesIfNoneMatch('', etag), false);
});

test('a missing object returns 503 with no stack trace', async () => {
  const missing = Object.assign(new Error('The specified key does not exist. at /var/task/index.js:1:2'), { name: 'NoSuchKey' });
  const { handler, logs } = harness(async () => { throw missing; });
  const response = await handler(event());
  assert.equal(response.statusCode, 503);
  assert.deepEqual(JSON.parse(response.body!), { error: 'registry_unavailable' });
  assert.equal(response.headers!['Cache-Control'], 'no-store');
  assert.ok(!response.body!.includes('at '), 'no stack in the response');
  assert.deepEqual(logs, ['registry read failed: NoSuchKey']);
  assert.ok(!logs.join('').includes('/var/task'), 'no stack or message in the log');
});

test('before the first publish (AccessDenied) and on any read error the answer is 503', async () => {
  for (const name of ['AccessDenied', 'TimeoutError']) {
    const { handler } = harness(async () => { throw Object.assign(new Error('x'), { name }); });
    assert.equal((await handler(event())).statusCode, 503, name);
  }
  const { handler } = harness(async () => { throw 'not an error'; });
  assert.equal((await handler(event())).statusCode, 503);
});

test('an object without a valid signature is never served', async () => {
  for (const stored of [
    { body, signature: undefined },
    { body, signature: '' },
    { body, signature: 'not-base64!' },
    { body, signature: Buffer.alloc(32, 1).toString('base64') },
    { body: new Uint8Array(), signature },
  ]) {
    const { handler, logs } = harness(async () => stored);
    const response = await handler(event());
    assert.equal(response.statusCode, 503);
    assert.equal(logs.length, 1);
  }
});
