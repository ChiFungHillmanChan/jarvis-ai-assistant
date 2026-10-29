import test from 'node:test';
import assert from 'node:assert/strict';
import { decide, hasCustomAttribute } from './decide.ts';

const native = { username: 'native-user-uuid', federated: false, verified: true };
const unverifiedNative = { username: 'unverified-native-uuid', federated: false, verified: false };
const google = { username: 'Google_1111', federated: true, verified: true };
const unverifiedGoogle = { username: 'Google_1111', federated: true, verified: false };

test('a Google sign-up for an email that already has a password account is linked to it', () => {
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', true, [native]), {
    action: 'link',
    destinationUsername: 'native-user-uuid',
    providerName: 'Google',
    providerUserId: '2222',
  });
});

test('Cognito may lowercase the provider prefix; the link still names the configured provider', () => {
  const d = decide('PreSignUp_ExternalProvider', 'google_3333', true, [native]);
  assert.equal(d.action, 'link');
  if (d.action === 'link') {
    assert.equal(d.providerName, 'Google');
    assert.equal(d.providerUserId, '3333');
  }
});

test('a first Google sign-up with no existing account is allowed', () => {
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', true, []), { action: 'allow' });
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', true, [google]), { action: 'allow' });
});

test('an unverified Google email is never linked, even to a verified native account', () => {
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', false, [native]), { action: 'allow' });
});

test('a verified Google sign-up is not linked to an unconfirmed native account', () => {
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', true, [unverifiedNative]), {
    action: 'allow',
  });
});

test('a verified Google sign-up links to the verified native account when both exist', () => {
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', true, [unverifiedNative, native]), {
    action: 'link',
    destinationUsername: 'native-user-uuid',
    providerName: 'Google',
    providerUserId: '2222',
  });
});

test('an email sign-up for an address Google already owns is rejected', () => {
  const d = decide('PreSignUp_SignUp', 'person@example.com', true, [google]);
  assert.equal(d.action, 'reject');
});

test('an email sign-up for an address an unverified Google account holds is allowed', () => {
  assert.deepEqual(decide('PreSignUp_SignUp', 'person@example.com', true, [unverifiedGoogle]), { action: 'allow' });
});

test('ordinary sign-ups and admin-created users pass through', () => {
  assert.deepEqual(decide('PreSignUp_SignUp', 'person@example.com', true, []), { action: 'allow' });
  assert.deepEqual(decide('PreSignUp_AdminCreateUser', 'x', true, [google]), { action: 'allow' });
});

test('sign-up is refused when it carries custom:tier', () => {
  assert.equal(hasCustomAttribute(Object.keys({ email: 'a@b.c', 'custom:tier': 'pro' })), true);
});

test('sign-up is refused when it carries custom:stripeCustomerId', () => {
  assert.equal(hasCustomAttribute(['custom:stripeCustomerId']), true);
});

test('sign-up is refused when a custom attribute is mixed with normal ones', () => {
  assert.equal(hasCustomAttribute(['email', 'email_verified', 'given_name', 'custom:tier']), true);
});

test('sign-up with only the standard attributes is allowed', () => {
  assert.equal(hasCustomAttribute(['email', 'email_verified', 'given_name']), false);
});

test('sign-up with no attributes is allowed', () => {
  assert.equal(hasCustomAttribute(Object.keys({})), false);
});
