import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { catalog } from '../src/i18n/catalog.ts';
import { GMAIL_NOT_GRANTED, GMAIL_WRONG_ACCOUNT, gmailConnectErrorKey, offersConnectGmail } from '../src/lib/google-gmail.ts';

// Gmail is opt-in for the Assistant: the first Google connect is Calendar
// only, and Settings offers Connect Gmail while the main account works
// without it.

test('Connect Gmail is offered only for a working main account without Gmail', () => {
  assert.equal(offersConnectGmail({ state: 'connected', gmail: false }), true);
  assert.equal(offersConnectGmail({ state: 'connected', gmail: true }), false);
  // Reconnecting comes first; nothing to add Gmail to when not connected.
  assert.equal(offersConnectGmail({ state: 'reconnect', gmail: false }), false);
  assert.equal(offersConnectGmail({ state: 'not_connected', gmail: false }), false);
});

test('the two Gmail refusals are explained in the interface language', () => {
  assert.equal(gmailConnectErrorKey(GMAIL_WRONG_ACCOUNT), 'googleGmailWrongAccount');
  assert.equal(gmailConnectErrorKey(GMAIL_NOT_GRANTED), 'googleGmailNotGranted');
  assert.equal(gmailConnectErrorKey('Token exchange failed: timeout'), null, 'anything else is shown as the backend said it');
  for (const key of ['googleGmailWrongAccount', 'googleGmailNotGranted', 'googleGmailOff', 'googleGmailOn', 'googleGmailConnect', 'googleGmailConnected'] as const) {
    assert.ok(catalog.en[key] && catalog['zh-HK'][key], key);
  }
});

test('the refusal sentinels match the Rust command word for word', () => {
  const rust = readFileSync(new URL('../src-tauri/src/commands/google_auth.rs', import.meta.url), 'utf8');
  assert.ok(rust.includes(`pub const GMAIL_WRONG_ACCOUNT: &str = "${GMAIL_WRONG_ACCOUNT}";`));
  assert.ok(rust.includes(`pub const GMAIL_NOT_GRANTED: &str = "${GMAIL_NOT_GRANTED}";`));
});
