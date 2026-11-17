import test from 'node:test';
import assert from 'node:assert/strict';
import { roundBreak } from '../src/workspace/chatState.ts';

test('a reply after a tool call starts a new paragraph instead of joining the last sentence', () => {
  assert.equal(roundBreak("I'll check your calendar.", 'Tomorrow'), '\n\n');
});

test('no break before the first words, after existing whitespace, or for a blank token', () => {
  assert.equal(roundBreak('', 'Tomorrow'), '');
  assert.equal(roundBreak('Checking.\n', 'Tomorrow'), '');
  assert.equal(roundBreak('Checking.', ' '), '');
});
