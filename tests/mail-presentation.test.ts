import test from 'node:test';
import assert from 'node:assert/strict';
import { fileSize, mailSourceUrl, parseSender, readableBody, revealMailSource } from '../src/lib/mail-presentation.ts';

test('Gmail source links target the message and its owning account', () => {
  const href = mailSourceUrl({ provider_id: '18afe12b09', account_email: 'first+work@example.test' });
  assert.notEqual(href, null, 'a synced Gmail message must offer its source');
  const url = new URL(href!);
  assert.equal(url.protocol, 'https:');
  assert.equal(url.hostname, 'mail.google.com');
  assert.equal(url.pathname, '/mail/');
  assert.equal(url.searchParams.get('authuser'), 'first+work@example.test');
  assert.equal(url.hash, '#all/18afe12b09');
  const second = new URL(mailSourceUrl({ provider_id: '18afe12b09', account_email: 'second@example.test' })!);
  assert.equal(second.searchParams.get('authuser'), 'second@example.test');
});

test('missing or malformed provider IDs and accounts cannot create misleading source links', () => {
  for (const provider_id of ['', '../settings', 'https://evil.test/', 'abc#inbox', 'msg-fixture', 'a'.repeat(65)]) {
    assert.equal(mailSourceUrl({ provider_id, account_email: 'work@example.test' }), null);
  }
  for (const account_email of ['', ' ', 'not-an-email', 'work@example.test\nother@example.test']) {
    assert.equal(mailSourceUrl({ provider_id: '18afe12b09', account_email }), null);
  }
});

test('an explicitly requested source scrolls into view before receiving focus', () => {
  const actions: unknown[] = [];
  const reader = {
    scrollIntoView: (options: unknown) => actions.push(['scroll', options]),
    focus: (options: unknown) => actions.push(['focus', options]),
  };
  assert.equal(revealMailSource(42, 42, reader), true);
  assert.deepEqual(actions, [
    ['scroll', { block: 'start', inline: 'nearest' }],
    ['focus', { preventScroll: true }],
  ]);
});

test('ordinary refreshes and a different or still-loading source do not move reading position', () => {
  let movements = 0;
  const reader = { scrollIntoView: () => { movements++; }, focus: () => { movements++; } };
  assert.equal(revealMailSource(null, 42, reader), false);
  assert.equal(revealMailSource(42, null, reader), false);
  assert.equal(revealMailSource(42, 99, reader), false);
  assert.equal(revealMailSource(42, 42, null), false);
  assert.equal(movements, 0);
});

test('senders show the display name and the address separately', () => {
  assert.deepEqual(parseSender('Brian Ko <brian@evoke.example>'), { name: 'Brian Ko', address: 'brian@evoke.example' });
  assert.deepEqual(parseSender('"Ko, Brian" <brian@evoke.example>'), { name: 'Ko, Brian', address: 'brian@evoke.example' });
  assert.deepEqual(parseSender('陳大文 <tai.man@example.hk>'), { name: '陳大文', address: 'tai.man@example.hk' });
  assert.deepEqual(parseSender('<noreply@example.com>'), { name: 'noreply@example.com', address: '' });
  assert.deepEqual(parseSender('client@example.com'), { name: 'client@example.com', address: '' });
  assert.deepEqual(parseSender('  '), { name: '', address: '' });
});

test('attachment sizes are readable', () => {
  assert.equal(fileSize(0), '');
  assert.equal(fileSize(512), '512 B');
  assert.equal(fileSize(184320), '180 KB');
  assert.equal(fileSize(3_250_000), '3.1 MB');
});

test('the reader does not repeat the headers already shown above the body', () => {
  const stored = 'From: Brian <b@example.com>\nTo: me@example.com\nDate: Tue, 22 Sep 2026 10:00:00 +0800\nSubject: Invoice\n\nPlease send the invoice.';
  assert.equal(readableBody(stored), 'Please send the invoice.');
  assert.equal(readableBody('Plain text that mentions From: someone'), 'Plain text that mentions From: someone');
  assert.equal(readableBody('From: only one header line\n\nbody'), 'From: only one header line\n\nbody');
});
