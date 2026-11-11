import test from 'node:test';
import assert from 'node:assert/strict';
import { replayableId, spokenMessage } from '../src/workspace/chatState.ts';

// A play button is only offered where there is a recording to fetch: a voice
// request the backend saved (it has an id) and marked as kept.

const message = (over: object) => ({ id: 7, role: 'user' as const, content: 'what is on my calendar', created_at: null, ...over });

test('a voice request with a kept recording can be played back', () => {
  assert.equal(replayableId(message({ has_recording: true })), 7);
});

test('typed messages, replies and unsaved messages offer no playback', () => {
  assert.equal(replayableId(message({})), null);
  assert.equal(replayableId(message({ has_recording: false })), null);
  assert.equal(replayableId(message({ has_recording: true, id: null })), null);
  assert.equal(replayableId(message({ has_recording: true, role: 'assistant' })), null);
});

test('a spoken request arriving live can be played back without reloading the chat', () => {
  const shown = spokenMessage({ role: 'user', content: 'what is on my calendar', id: 12, has_recording: true }, '2026-11-11T12:00:00.000Z');
  assert.deepEqual(shown, { id: 12, role: 'user', content: 'what is on my calendar', created_at: '2026-11-11T12:00:00.000Z', has_recording: true });
  assert.equal(replayableId(shown), 12);
});

test('a live voice event without a recording is shown as an ordinary message', () => {
  const shown = spokenMessage({ role: 'assistant', content: 'You have two meetings.' }, '2026-11-11T12:00:00.000Z');
  assert.deepEqual(shown, { id: null, role: 'assistant', content: 'You have two meetings.', created_at: '2026-11-11T12:00:00.000Z', has_recording: false });
  assert.equal(replayableId(shown), null);
});
