import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { loadTs } from '../../test-support/load-ts.cjs';

// The handler builds its AWS clients when it loads, so fake both before loading it. No AWS call is made.
const require = createRequire(import.meta.url);
const { DynamoDBDocumentClient } = require('@aws-sdk/lib-dynamodb');
const { CognitoIdentityProviderClient } = require('@aws-sdk/client-cognito-identity-provider');

process.env.USER_META_TABLE = 'user-meta';
process.env.AI_GRANTS_TABLE = 'grants';
process.env.AI_CONFIG_TABLE = 'config';
process.env.AI_PERIOD_TABLE = 'period';
process.env.USER_POOL_ID = 'ap-southeast-1_fixture';

const failing = new Set<string>();
const fakeDdb = {
  async send(command: any) {
    const table: string = command.input.TableName;
    if (failing.has(table)) throw new Error('ddb unavailable');
    if (table === 'user-meta' && command.constructor.name === 'GetCommand') return { Item: { userId: 'u1', tier: 'free' } };
    if (table === 'grants') {
      return { Items: [{ grantId: 'g1', type: 'trial', creditMicros: 5_000_000, validFrom: '2000-01-01T00:00:00.000Z', validUntil: '2999-01-01T00:00:00.000Z', createdAt: '2026-01-01T00:00:00.000Z' }] };
    }
    if (table === 'period') return { Item: { billedMicros: 1234 } };
    return {};
  },
};
DynamoDBDocumentClient.from = () => fakeDdb;
CognitoIdentityProviderClient.prototype.send = async () => ({ UserAttributes: [{ Name: 'email', Value: 'a@example.com' }] });

const { handler } = loadTs('lambda/me/index.ts');
const event: any = { requestContext: { authorizer: { jwt: { claims: { sub: 'u1', username: 'u1' } } } }, headers: {} };

async function me(): Promise<any> {
  const res = await handler(event);
  assert.equal(res.statusCode, 200);
  return JSON.parse(res.body);
}

test('with every read working, /me reports the AI status', async () => {
  failing.clear();
  const body = await me();
  assert.equal(body.jarvisAi.entitled, true);
  assert.equal(body.jarvisAi.usedMicros, 1234);
});

test('if the config, grants or period read fails, /me is still 200 with jarvisAi null and the rest unchanged', async () => {
  failing.clear();
  const { jarvisAi: _ok, ...rest } = await me();
  for (const table of ['config', 'grants', 'period']) {
    failing.clear();
    failing.add(table);
    const body = await me();
    assert.equal(body.jarvisAi, null, table);
    const { jarvisAi: _null, ...others } = body;
    assert.deepEqual(others, rest, table);
  }
  failing.clear();
});

test('a failing user-meta read still fails /me', async () => {
  failing.clear();
  failing.add('user-meta');
  await assert.rejects(handler(event));
  failing.clear();
});
