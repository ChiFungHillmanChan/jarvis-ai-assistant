import test from 'node:test';
import assert from 'node:assert/strict';
import { decide } from './decide.ts';

const native = { username: 'native-user-uuid', federated: false };
const google = { username: 'Google_1111', federated: true };

test('a Google sign-up for an email that already has a password account is linked to it', () => {
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', [native]), {
    action: 'link',
    destinationUsername: 'native-user-uuid',
    providerName: 'Google',
    providerUserId: '2222',
  });
});

test('Cognito may lowercase the provider prefix; the link still names the configured provider', () => {
  const d = decide('PreSignUp_ExternalProvider', 'google_3333', [native]);
  assert.equal(d.action, 'link');
  if (d.action === 'link') {
    assert.equal(d.providerName, 'Google');
    assert.equal(d.providerUserId, '3333');
  }
});

test('a first Google sign-up with no existing account is allowed', () => {
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', []), { action: 'allow' });
  assert.deepEqual(decide('PreSignUp_ExternalProvider', 'Google_2222', [google]), { action: 'allow' });
});

test('an email sign-up for an address Google already owns is rejected', () => {
  const d = decide('PreSignUp_SignUp', 'person@example.com', [google]);
  assert.equal(d.action, 'reject');
});

test('ordinary sign-ups and admin-created users pass through', () => {
  assert.deepEqual(decide('PreSignUp_SignUp', 'person@example.com', []), { action: 'allow' });
  assert.deepEqual(decide('PreSignUp_AdminCreateUser', 'x', [google]), { action: 'allow' });
});
