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

// --- playing one back -------------------------------------------------------

import { createReplay } from '../src/lib/voice-replay.ts';

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}
const settle = () => new Promise(resolve => setImmediate(resolve));

function harness() {
  const load = deferred<ArrayBuffer>();
  const decode = deferred<unknown>();
  const states: string[] = [];
  const audio = { made: 0, closed: 0, started: 0, ended: () => {} };
  const replay = createReplay(
    () => load.promise,
    () => {
      audio.made++;
      return {
        decode: () => decode.promise,
        play: (_buffer: unknown, onEnded: () => void) => { audio.started++; audio.ended = onEnded; },
        close: () => { audio.closed++; },
      };
    },
    state => states.push(state),
  );
  return { load, decode, states, audio, replay };
}

test('a recording plays, and the button goes back when it ends', async () => {
  const h = harness();
  void h.replay.toggle();
  h.load.resolve(new ArrayBuffer(8)); await settle();
  h.decode.resolve({}); await settle();
  assert.equal(h.audio.started, 1);
  assert.equal(h.states.at(-1), 'playing');
  h.audio.ended();
  assert.equal(h.states.at(-1), 'idle');
  assert.equal(h.audio.closed, 1, 'the audio device is released when playback ends');
});

test('pressing the button while it plays stops it', async () => {
  const h = harness();
  void h.replay.toggle();
  h.load.resolve(new ArrayBuffer(8)); await settle();
  h.decode.resolve({}); await settle();
  void h.replay.toggle();
  assert.equal(h.states.at(-1), 'idle');
  assert.equal(h.audio.closed, 1);
  h.audio.ended();
  assert.equal(h.audio.closed, 1, 'the end of a stopped recording changes nothing');
});

test('pressing again while it is still being prepared cancels it', async () => {
  const h = harness();
  void h.replay.toggle();
  h.load.resolve(new ArrayBuffer(8)); await settle();
  void h.replay.toggle();
  h.decode.resolve({}); await settle();
  assert.equal(h.audio.started, 0, 'a cancelled recording must not start');
  assert.ok(!h.states.includes('playing'), 'the button must not say Stop with nothing playing');
  assert.equal(h.audio.closed, 1);
});

test('leaving the chat before the recording arrives plays nothing', async () => {
  const h = harness();
  void h.replay.toggle();
  h.replay.stop();
  h.load.resolve(new ArrayBuffer(8)); await settle();
  assert.equal(h.audio.made, 0);
  assert.equal(h.audio.started, 0);
});

test('a recording that cannot be fetched or decoded says so and can be retried', async () => {
  const h = harness();
  void h.replay.toggle();
  h.load.reject(new Error('gone')); await settle();
  assert.equal(h.states.at(-1), 'failed');
  void h.replay.toggle();
  assert.equal(h.states.at(-1), 'idle', 'a retry clears the failure');
});
