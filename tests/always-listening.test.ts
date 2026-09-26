import test from 'node:test';
import assert from 'node:assert/strict';
import { canSwitchListening, setAlwaysListening, voiceStateKind, type ListeningApi } from '../src/lib/always-listening.ts';

function fakeMac(modelDownloaded: boolean) {
  const calls: string[] = [];
  const state = { enabled: false, running: false, model_downloaded: modelDownloaded };
  const api: ListeningApi = {
    status: async () => ({ ...state, voice_state: 'Idle' }),
    // A real download takes time; finishing on a later tick is what makes a missing await fail.
    download: async () => { calls.push('download'); await new Promise(resolve => setTimeout(resolve, 0)); state.model_downloaded = true; },
    enable: async () => {
      calls.push('enable');
      if (!state.model_downloaded) throw new Error('Wake-word model not downloaded');
      state.enabled = state.running = true;
    },
    disable: async () => { calls.push('disable'); state.enabled = state.running = false; },
  };
  return { api, calls };
}

test('turning listening on for the first time downloads the speech model before starting', async () => {
  const mac = fakeMac(false);
  const status = await setAlwaysListening(true, mac.api);
  assert.deepEqual(mac.calls, ['download', 'enable']);
  assert.equal(status.running, true);
});

test('turning listening on with the model present starts without downloading again', async () => {
  const mac = fakeMac(true);
  await setAlwaysListening(true, mac.api);
  assert.deepEqual(mac.calls, ['enable']);
});

test('turning listening off stops it and reports the new state', async () => {
  const mac = fakeMac(true);
  await setAlwaysListening(true, mac.api);
  const status = await setAlwaysListening(false, mac.api);
  assert.deepEqual(mac.calls, ['enable', 'disable']);
  assert.equal(status.running, false);
  assert.equal(status.enabled, false);
});

test('the switch waits while the model downloads or a push-to-talk recording runs', () => {
  const on = { enabled: true, running: true, model_downloaded: true, voice_state: 'WakeWordListening' as const };
  assert.equal(canSwitchListening(on, false, 'WakeWordListening'), true);
  assert.equal(canSwitchListening(on, false, 'Idle'), true);
  assert.equal(canSwitchListening(null, false, 'Idle'), false, 'status not loaded yet');
  assert.equal(canSwitchListening(on, true, 'Idle'), false, 'a switch already in progress');
  assert.equal(canSwitchListening(on, false, { ModelDownloading: 60 }), false, 'a second download would corrupt the model');
  for (const recording of ['Listening', 'Processing', 'Speaking'] as const) {
    assert.equal(canSwitchListening(on, false, recording), false, `${recording} belongs to push-to-talk`);
  }
});

test('status is re-read when the voice state changes kind, not on every download percent', () => {
  assert.equal(voiceStateKind({ ModelDownloading: 5 }), voiceStateKind({ ModelDownloading: 60 }));
  assert.notEqual(voiceStateKind('WakeWordListening'), voiceStateKind('Idle'));
  assert.notEqual(voiceStateKind({ Error: 'mic' }), voiceStateKind('Idle'));
});
