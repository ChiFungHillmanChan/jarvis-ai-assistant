import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { setupChecklist, type ChecklistInput } from '../src/settings/setup-checklist.ts';

// Task 7: the Setup guide's checklist. Four items, always in the same order,
// each done exactly when its input says so.

const none: ChecklistInput = { aiReady: false, googleConnected: false, autoTriage: false, briefingTime: null };

test('setupChecklist returns the four items in order', () => {
  assert.deepEqual(setupChecklist(none).map(item => item.id), ['ai', 'google', 'triage', 'briefing']);
});

test('nothing is done when nothing is set up', () => {
  assert.deepEqual(setupChecklist(none).map(item => item.done), [false, false, false, false]);
});

test('everything is done when everything is set up', () => {
  const all = setupChecklist({ aiReady: true, googleConnected: true, autoTriage: true, briefingTime: '08:30' });
  assert.deepEqual(all.map(item => item.done), [true, true, true, true]);
});

test('each item follows only its own input', () => {
  const done = (input: Partial<ChecklistInput>) => setupChecklist({ ...none, ...input }).filter(item => item.done).map(item => item.id);
  assert.deepEqual(done({ aiReady: true }), ['ai']);
  assert.deepEqual(done({ googleConnected: true }), ['google']);
  assert.deepEqual(done({ autoTriage: true }), ['triage']);
  assert.deepEqual(done({ briefingTime: '07:00' }), ['briefing']);
});

test('a blank briefing time is not a set time', () => {
  assert.equal(setupChecklist({ ...none, briefingTime: '' })[3].done, false);
  assert.equal(setupChecklist({ ...none, briefingTime: '   ' })[3].done, false);
});

test('the checklist helper imports no React and no .tsx', () => {
  const code = readFileSync(new URL('../src/settings/setup-checklist.ts', import.meta.url), 'utf8');
  assert.doesNotMatch(code, /from ['"]react['"]/);
  assert.doesNotMatch(code, /\.tsx['"]/);
});
