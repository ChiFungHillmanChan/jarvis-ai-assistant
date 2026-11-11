import test from 'node:test';
import assert from 'node:assert/strict';
import { paymentTemplate } from '../../test-support/payment-template.cjs';

// The Stripe key and webhook signing secret are read only by the three billing
// Lambdas. An administrator would first have to change this policy, which
// CloudTrail records, before reading them.
test('only the billing functions can read the Stripe secret', () => {
  const template = paymentTemplate();
  const resources = Object.entries(template.Resources) as [string, any][];
  const secretId = resources.find(([, r]) => r.Type === 'AWS::SecretsManager::Secret')?.[0];
  assert.ok(secretId, 'Payment stack declares the Stripe secret');
  const policy = resources.find(([, r]) => r.Type === 'AWS::SecretsManager::ResourcePolicy')?.[1];
  assert.ok(policy, 'the Stripe secret has a resource policy');
  assert.deepEqual(policy.Properties.SecretId, { Ref: secretId });
  assert.equal(policy.Properties.BlockPublicPolicy, true);

  const deny = policy.Properties.ResourcePolicy.Statement.find((s: any) => s.Effect === 'Deny');
  assert.ok(deny, 'reading is denied by default');
  assert.deepEqual(deny.Principal, { AWS: '*' });
  assert.ok([deny.Action].flat().includes('secretsmanager:GetSecretValue'));
  const allowed = deny.Condition.ArnNotEquals['aws:PrincipalArn'].map((arn: any) => arn['Fn::GetAtt']?.[0]);
  const roleOf = (name: string) => resources.find(([id, r]) => r.Type === 'AWS::IAM::Role' && id.startsWith(name))?.[0];
  const readers = ['StripeWebhookFn', 'StripeCheckoutFn', 'StripePortalFn'].map(roleOf);
  assert.ok(readers.every(Boolean), 'each billing function has its own role');
  assert.deepEqual([...allowed].sort(), [...readers].sort());
  assert.equal(roleOf('MeFn') && allowed.includes(roleOf('MeFn')), false, '/me never needs the Stripe key');
});
