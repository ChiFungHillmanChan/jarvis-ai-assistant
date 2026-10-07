import test from 'node:test';
import assert from 'node:assert/strict';
import { quoteAuthor, quoteThread, splitQuotes, type MailQuote } from '../src/components/content/mail-quotes.ts';

// Gmail's text/plain reply: new text, a top-level attribution, then ">" and ">>" history
// whose nested attribution wraps onto a second quoted line.
const reply = [
  'For clarity, this email does not accept the proposed addendum.',
  '',
  'Best regards,',
  'Hillman',
  '',
  'On Tue, 6 Oct 2026 at 13:37, Yu from RAWR <rawrproject.hk@gmail.com> wrote:',
  '',
  '> Hi Hillman,',
  '>',
  '> Thank you for setting out the revised lean programme.',
  '>',
  '> Best,',
  '> Yu',
  '>',
  '> On Mon, 5 Oct 2026 at 11:45 PM, Hillman Chan <hillmanchan709@gmail.com>',
  '> wrote:',
  '>',
  '>> Hi Yu,',
  '>>',
  '>> Here is the proposal.',
].join('\r\n');

test('a Gmail reply separates the new text from each quoted message and its attribution', () => {
  assert.deepEqual(splitQuotes(reply), [
    { type: 'text', text: 'For clarity, this email does not accept the proposed addendum.\n\nBest regards,\nHillman' },
    { type: 'quote', attribution: 'On Tue, 6 Oct 2026 at 13:37, Yu from RAWR <rawrproject.hk@gmail.com> wrote:', parts: [
      { type: 'text', text: 'Hi Hillman,\n\nThank you for setting out the revised lean programme.\n\nBest,\nYu' },
      { type: 'quote', attribution: 'On Mon, 5 Oct 2026 at 11:45 PM, Hillman Chan <hillmanchan709@gmail.com> wrote:', parts: [
        { type: 'text', text: 'Hi Yu,\n\nHere is the proposal.' },
      ] },
    ] },
  ]);
});

test('spaced "> >" markers nest like ">>" and only one marker is removed per level', () => {
  assert.deepEqual(splitQuotes('Reply\n> one\n> > two\n>>   indented'), [
    { type: 'text', text: 'Reply' },
    { type: 'quote', attribution: null, parts: [
      { type: 'text', text: 'one' },
      { type: 'quote', attribution: null, parts: [{ type: 'text', text: 'two\n  indented' }] },
    ] },
  ]);
});

test('text that merely starts with ">" stays as written', () => {
  assert.deepEqual(splitQuotes('>50% of users\n->arrow'), [{ type: 'text', text: '>50% of users\n->arrow' }]);
});

test('an ordinary sentence ending in "wrote:" becomes the header of the quote it introduces, alone', () => {
  const [, quote] = splitQuotes('I agree with this.\nAs Yu wrote:\n> keep it lean') as [unknown, MailQuote];
  assert.equal(quote.attribution, 'As Yu wrote:');
  assert.deepEqual(splitQuotes('I agree with this.\nAs Yu wrote:\n> keep it lean')[0], { type: 'text', text: 'I agree with this.' });
});

test('a mail with no quoted lines is a single unchanged text part', () => {
  assert.deepEqual(splitQuotes('Line one\n\n  indented line'), [{ type: 'text', text: 'Line one\n\n  indented line' }]);
});

test('attribution lines name the sender, address and date in English and Chinese Gmail', () => {
  assert.deepEqual(quoteAuthor('On Tue, 6 Oct 2026 at 13:37, Yu from RAWR <rawrproject.hk@gmail.com> wrote:'),
    { name: 'Yu from RAWR', address: 'rawrproject.hk@gmail.com', date: 'Tue, 6 Oct 2026 at 13:37' });
  assert.deepEqual(quoteAuthor('On 6 Oct 2026, at 13:37, Yu <yu@example.test> wrote:'),
    { name: 'Yu', address: 'yu@example.test', date: '6 Oct 2026, at 13:37' });
  assert.deepEqual(quoteAuthor('On Tue, Oct 6, 2026 at 1:37 PM, "Chan, Hillman" wrote:'),
    { name: 'Chan, Hillman', address: null, date: 'Tue, Oct 6, 2026 at 1:37 PM' });
  assert.deepEqual(quoteAuthor('On Tue, 6 Oct 2026 at 13:37, <yu@example.test> wrote:'),
    { name: 'yu@example.test', address: 'yu@example.test', date: 'Tue, 6 Oct 2026 at 13:37' });
  assert.deepEqual(quoteAuthor('Yu from RAWR <rawrproject.hk@gmail.com> 於 2026年10月6日 週二 下午1:37寫道：'),
    { name: 'Yu from RAWR', address: 'rawrproject.hk@gmail.com', date: '2026年10月6日 週二 下午1:37' });
  assert.deepEqual(quoteAuthor('Yu <yu@example.test> 于2026年10月6日周二 13:37写道：'),
    { name: 'Yu', address: 'yu@example.test', date: '2026年10月6日周二 13:37' });
  assert.equal(quoteAuthor('As Yu wrote:'), null);
});

test('a top-posted history reads as a flat list of earlier messages, newest first', () => {
  const [, trail] = splitQuotes(reply) as [unknown, MailQuote];
  const thread = quoteThread(trail);
  assert.deepEqual(thread.map(q => q.attribution?.slice(0, 15)), ['On Tue, 6 Oct 2', 'On Mon, 5 Oct 2']);
  assert.deepEqual(thread[0].parts, [{ type: 'text', text: 'Hi Hillman,\n\nThank you for setting out the revised lean programme.\n\nBest,\nYu' }]);
  // An unattributed inner quote is a quotation inside that message, so it stays nested.
  const inline = splitQuotes('> Reply\n>> quoted line')[0] as MailQuote;
  assert.equal(quoteThread(inline).length, 1);
});
