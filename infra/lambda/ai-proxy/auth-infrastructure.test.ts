import test from 'node:test';
import assert from 'node:assert/strict';
import { synthStack } from '../../test-support/stack-template.cjs';

const auth = () => synthStack('lib/auth-stack.ts', 'AuthStack', () => ({
  envName: 'sandbox',
  appDomain: 'example.test',
}));

const appClients = (template: any): [string, any][] =>
  Object.entries(template.Resources).filter(([, r]: [string, any]) => r.Type === 'AWS::Cognito::UserPoolClient') as [string, any][];

test('the app client signs in only through the hosted UI, refreshing with its token', () => {
  const [[, client]] = appClients(auth());
  assert.deepEqual([...client.Properties.ExplicitAuthFlows].sort(), ['ALLOW_REFRESH_TOKEN_AUTH']);
  assert.ok(!client.Properties.ExplicitAuthFlows.includes('ALLOW_USER_SRP_AUTH'));
});

test('the app client can write only the attributes the Google provider maps', () => {
  const [[, client]] = appClients(auth());
  assert.deepEqual([...client.Properties.WriteAttributes].sort(), ['email', 'email_verified', 'given_name']);
});

test('the app client is updated in place, not replaced', () => {
  const clients = appClients(auth());
  assert.equal(clients.length, 1);
  assert.equal(clients[0][0], 'JarvisUserPoolV2JarvisAppClientFC5DE69D');
  assert.equal(clients[0][1].Properties.ClientName, 'jarvis-sandbox-app');
  assert.equal(clients[0][1].Properties.GenerateSecret, undefined);
});
