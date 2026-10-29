import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { loadTs } from '../../test-support/load-ts.cjs';

// The handler builds its token verifier and deps when it loads, so stub them first. No AWS call is made.
const { CognitoJwtVerifier } = createRequire(import.meta.url)('aws-jwt-verify');
CognitoJwtVerifier.create = () => ({ verify: async () => ({ sub: 'u1' }) });
(globalThis as any).awslambda = {
  streamifyResponse: (handler: any) => handler,
  HttpResponseStream: { from: () => ({ write() {}, end() {}, on() {} }) },
};
process.env.USER_POOL_ID = 'ap-southeast-1_fixture';
process.env.USER_POOL_CLIENT_ID = 'fixture-client';

const callers: any[] = [];
loadTs('lambda/ai-proxy/converse.ts').converse = async (_deps: any, caller: any) => { callers.push(caller); };
const { handler } = loadTs('lambda/ai-proxy/index.ts');

const event = (version?: string) => ({
  requestContext: { http: { method: 'POST' } },
  rawPath: '/v1/converse',
  body: '{}',
  isBase64Encoded: false,
  headers: { authorization: 'Bearer t', ...(version === undefined ? {} : { 'x-jarvis-app-version': version }) },
});

test('the proxy keeps only an app version that /me would also accept', async () => {
  const cases: [string | undefined, string | null][] = [
    ['0.6.0', '0.6.0'],
    ['0.6.0-beta.1', '0.6.0-beta.1'],
    ['hello world', null],
    ['me@example.com', null],
    ['1.2', null],
    ['', null],
    [`1.2.3-${'a'.repeat(21)}`, null],
    [undefined, null],
  ];
  for (const [header, expected] of cases) {
    callers.length = 0;
    await handler(event(header), {});
    assert.deepEqual(callers, [{ userId: 'u1', appVersion: expected }], String(header));
  }
});
