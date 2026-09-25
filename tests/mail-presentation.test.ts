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

import { backendUtc, decodeEntities, defaultRemindAt, dueBucket, formatDue } from '../src/lib/mail-presentation.ts';

test('snippets saved before the sync fix are shown decoded', () => {
  assert.equal(decodeEntities('Here&#39;s your digital label &amp; instructions'), "Here's your digital label & instructions");
  assert.equal(decodeEntities('&lt;b&gt; &#x4E2D;&#25991; &quot;x&quot;'), '<b> 中文 "x"');
  assert.equal(decodeEntities('AT&T &unknown; &#0;'), 'AT&T &unknown; &#0;');
});

test('agenda buckets follow the local date', () => {
  const today = new Date('2026-09-25T10:00:00');
  assert.equal(dueBucket('2026-09-24', today), 'overdue');
  assert.equal(dueBucket('2026-09-25', today), 'today');
  assert.equal(dueBucket('2026-10-02', today), 'week');
  assert.equal(dueBucket('2026-10-03', today), 'later');
  assert.equal(dueBucket(null, today), 'undated');
});

test('the first reminder is before the item and never in the past', () => {
  const now = new Date('2026-09-25T10:20:00');
  assert.equal(defaultRemindAt('2026-09-30', '15:00', now), '2026-09-30T14:00');
  assert.equal(defaultRemindAt('2026-09-30', null, now), '2026-09-30T09:00');
  assert.equal(defaultRemindAt(null, null, now), '2026-09-26T09:00');
  // Due in 30 minutes: an hour before is already past, so remind next hour.
  assert.equal(defaultRemindAt('2026-09-25', '10:50', now), '2026-09-25T11:00');
});

test('backend UTC timestamps are not read as local time', () => {
  assert.equal(backendUtc('2026-09-25 18:01:23').toISOString(), '2026-09-25T18:01:23.000Z');
  assert.equal(backendUtc('2026-09-25T18:01:23Z').toISOString(), '2026-09-25T18:01:23.000Z');
  assert.ok(formatDue('2026-09-30', '15:00', 'en-GB').endsWith('15:00'));
  assert.equal(formatDue(null, null, 'en-GB'), '');
});

test('due dates read naturally in both languages', () => {
  assert.equal(formatDue('2026-09-27', null, 'zh-HK'), '9月27日（週日）');
  assert.equal(formatDue('2026-09-27', '15:00', 'zh-HK', false), '9月27日 15:00');
  assert.match(formatDue('2026-09-27', null, 'en-GB'), /Sun/);
});
