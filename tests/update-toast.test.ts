import test from 'node:test';
import assert from 'node:assert/strict';
import { initialToastState, isToastVisible, updateToastReducer } from '../src/lib/update.ts';

// The toast is the only update affordance in the sidebar, so the rules that
// decide when it is on screen are the rules that decide whether a user can
// update at all. They live here rather than inside the component.

const reduce = (events: Parameters<typeof updateToastReducer>[1][]) =>
  events.reduce(updateToastReducer, initialToastState);

test('nothing is shown until a check finds a version', () => {
  assert.equal(isToastVisible(initialToastState), false);
  assert.equal(isToastVisible(reduce([{ type: 'checked', version: null }])), false);
});

test('a found version shows the toast with that version', () => {
  const state = reduce([{ type: 'checked', version: '0.5.0' }]);
  assert.equal(isToastVisible(state), true);
  assert.equal(state.version, '0.5.0');
  assert.equal(state.phase, 'idle');
});

test('dismissing hides the toast', () => {
  const state = reduce([{ type: 'checked', version: '0.5.0' }, { type: 'dismissed' }]);
  assert.equal(isToastVisible(state), false);
});

test('the next check brings a dismissed toast back for the same version', () => {
  // Dismissal is for this sitting only. Without this the user dismisses once
  // and the app can never offer the update again.
  const state = reduce([
    { type: 'checked', version: '0.5.0' },
    { type: 'dismissed' },
    { type: 'checked', version: '0.5.0' },
  ]);
  assert.equal(isToastVisible(state), true);
});

test('a check that no longer finds an update clears the toast', () => {
  const state = reduce([{ type: 'checked', version: '0.5.0' }, { type: 'checked', version: null }]);
  assert.equal(isToastVisible(state), false);
  assert.equal(state.version, null);
});

test('a download in progress stays visible even if dismissed', () => {
  const state = reduce([
    { type: 'checked', version: '0.5.0' },
    { type: 'download-started' },
    { type: 'dismissed' },
  ]);
  assert.equal(isToastVisible(state), true);
  assert.equal(state.phase, 'downloading');
});

test('progress is recorded while downloading', () => {
  const state = reduce([
    { type: 'checked', version: '0.5.0' },
    { type: 'download-started' },
    { type: 'progress', percent: 42 },
  ]);
  assert.equal(state.progress, 42);
});

test('a check during a download does not replace the version being installed', () => {
  // The updater handle is already downloading; swapping the version underneath
  // it would relabel the toast mid-install.
  const state = reduce([
    { type: 'checked', version: '0.5.0' },
    { type: 'download-started' },
    { type: 'checked', version: '0.6.0' },
  ]);
  assert.equal(state.version, '0.5.0');
  assert.equal(state.phase, 'downloading');
});

test('a failed download stays visible so it can be retried', () => {
  const state = reduce([
    { type: 'checked', version: '0.5.0' },
    { type: 'download-started' },
    { type: 'failed', message: 'network unreachable' },
    { type: 'dismissed' },
  ]);
  assert.equal(isToastVisible(state), true);
  assert.equal(state.phase, 'error');
  assert.equal(state.message, 'network unreachable');
});

test('retrying after a failure clears the previous error', () => {
  const state = reduce([
    { type: 'checked', version: '0.5.0' },
    { type: 'download-started' },
    { type: 'failed', message: 'network unreachable' },
    { type: 'download-started' },
  ]);
  assert.equal(state.phase, 'downloading');
  assert.equal(state.message, null);
  assert.equal(state.progress, 0);
});
