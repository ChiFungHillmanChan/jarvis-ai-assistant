import test from 'node:test';
import assert from 'node:assert/strict';
import { readTrialDays } from './trial.ts';

test('an unset trial length is the 28-day default', () => {
  assert.equal(readTrialDays(undefined), 28);
  assert.equal(readTrialDays(''), 28);
});

test('a whole number of days from 1 to 730 is accepted', () => {
  assert.equal(readTrialDays('1'), 1);
  assert.equal(readTrialDays(' 61 '), 61);
  assert.equal(readTrialDays('730'), 730);
});

test('anything else is refused rather than guessed', () => {
  for (const raw of ['0', '731', '2.5', '-3', 'four', '28 days']) assert.equal(readTrialDays(raw), null, raw);
});
