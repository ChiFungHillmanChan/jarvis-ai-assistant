import test from 'node:test';
import assert from 'node:assert/strict';
import { setAlwaysListening, type ListeningApi } from '../src/lib/always-listening.ts';

function fakeMac(modelDownloaded: boolean) {
  const calls: string[] = [];
  const state = { enabled: false, running: false, model_downloaded: modelDownloaded };
  const api: ListeningApi = {
    status: async () => ({ ...state, voice_state: 'Idle' }),
    download: async () => { calls.push('download'); state.model_downloaded = true; },
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
