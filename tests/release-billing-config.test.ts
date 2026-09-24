import test from 'node:test';
import assert from 'node:assert/strict';
import { validateBillingReleaseConfig } from '../scripts/release-billing-config.mjs';

const configured = {
  JARVIS_PAYMENT_API: 'https://payments.example.test',
  JARVIS_COGNITO_DOMAIN: 'https://auth.example.test',
  JARVIS_COGNITO_CLIENT_ID: 'publicclient123',
};

test('a payment endpoint alone cannot ship a release with a hidden account entry', () => {
  assert.deepEqual(validateBillingReleaseConfig({ JARVIS_PAYMENT_API: configured.JARVIS_PAYMENT_API }), [
    'JARVIS_COGNITO_DOMAIN', 'JARVIS_COGNITO_CLIENT_ID',
  ]);
});

test('all three public billing settings are required for a downloadable release', () => {
  assert.deepEqual(validateBillingReleaseConfig(configured), []);
  for (const key of Object.keys(configured)) {
    for (const value of ['', ' ', 'None', 'null', 'undefined']) {
      assert.ok(validateBillingReleaseConfig({ ...configured, [key]: value }).includes(key));
    }
  }
});

test('billing release endpoints must be HTTPS bases without credentials or return parameters', () => {
  for (const key of ['JARVIS_PAYMENT_API', 'JARVIS_COGNITO_DOMAIN']) {
    for (const value of ['http://example.test', 'https://user:secret@example.test', 'https://example.test?token=secret', 'https://example.test/#return', 'not a url']) {
      assert.ok(validateBillingReleaseConfig({ ...configured, [key]: value }).includes(key));
    }
  }
  assert.deepEqual(validateBillingReleaseConfig({ ...configured, JARVIS_PAYMENT_API: 'https://example.test/v1/', JARVIS_COGNITO_DOMAIN: 'auth.example.test' }), []);
});

test('invalid configuration reports variable names without leaking their values', () => {
  const invalid = { ...configured, JARVIS_PAYMENT_API: 'sensitive-invalid-value' };
  assert.deepEqual(validateBillingReleaseConfig(invalid), ['JARVIS_PAYMENT_API']);
});

test('raw endpoint syntax must survive the desktop URL concatenation', () => {
  for (const key of ['JARVIS_PAYMENT_API', 'JARVIS_COGNITO_DOMAIN']) {
    for (const value of ['https://example.test/?', 'https://example.test/#', 'https://example.test/\n', 'https://exam\nple.test']) {
      assert.ok(validateBillingReleaseConfig({ ...configured, [key]: value }).includes(key), `${key}: ${JSON.stringify(value)}`);
    }
  }
  assert.deepEqual(validateBillingReleaseConfig({ ...configured, JARVIS_COGNITO_DOMAIN: 'HTTPS://auth.example.test' }), ['JARVIS_COGNITO_DOMAIN']);
});
