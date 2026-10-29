import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { loadTs } from '../../test-support/load-ts.cjs';

// The handler builds its token verifier and deps when it loads, so stub them first. No AWS call is made.
const { CognitoJwtVerifier } = createRequire(import.meta.url)('aws-jwt-verify');
const jwtErrors = createRequire(import.meta.url)('aws-jwt-verify/error');
let verifyImpl: () => Promise<{ sub: string }> = async () => ({ sub: 'u1' });
CognitoJwtVerifier.create = () => ({ verify: () => verifyImpl() });
const responses: { status: number; body: string }[] = [];
(globalThis as any).awslambda = {
  streamifyResponse: (handler: any) => handler,
  HttpResponseStream: {
    from: (_stream: any, meta: { statusCode: number }) => {
      const response = { status: meta.statusCode, body: '' };
      responses.push(response);
      return { write(chunk: string) { response.body += chunk; }, end() {}, on() {} };
    },
  },
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

async function denied(): Promise<{ status: number; body: unknown }> {
  responses.length = 0;
  callers.length = 0;
  await handler(event(), {});
  assert.deepEqual(callers, [], 'the request must not reach converse');
  assert.equal(responses.length, 1);
  return { status: responses[0].status, body: JSON.parse(responses[0].body) };
}

test('signing keys that cannot be fetched are a 503, not a 401', async () => {
  const fetchFailures = [
    new jwtErrors.FetchError('https://cognito-idp.example/jwks.json', 'Response time-out'),
    new jwtErrors.NonRetryableFetchError('https://cognito-idp.example/jwks.json', 'Status code is 403, expected 200'),
    new jwtErrors.JwksNotAvailableInCacheError('JWKS not yet available in cache'),
  ];
  for (const error of fetchFailures) {
    verifyImpl = async () => { throw error; };
    assert.deepEqual(await denied(), { status: 503, body: { error: 'upstream_error' } }, error.constructor.name);
  }
});

test('a bad token stays a 401', async () => {
  const badTokens = [
    new jwtErrors.JwtParseError('not a jwt'),
    new jwtErrors.JwtInvalidSignatureError('bad signature'),
    new jwtErrors.JwtExpiredError('expired', 0),
    new jwtErrors.CognitoJwtInvalidClientIdError('wrong client', 'x', 'y'),
    new jwtErrors.CognitoJwtInvalidTokenUseError('wrong token_use', 'id', ['access']),
    new jwtErrors.KidNotFoundInJwksError('unknown kid'),
    new Error('anything else'),
  ];
  for (const error of badTokens) {
    verifyImpl = async () => { throw error; };
    assert.deepEqual(await denied(), { status: 401, body: { error: 'unauthenticated' } }, error.constructor.name);
  }
  verifyImpl = async () => ({ sub: 'u1' });
});
