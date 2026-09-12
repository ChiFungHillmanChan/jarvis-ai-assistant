import test from 'node:test';
import assert from 'node:assert/strict';
import { followJob, canStartFirstAnalysis } from '../src/lib/setup-flow.ts';

test('job progress ignores an old job and waits for the matching terminal event', async () => {
  let deliver: (event: { jobId: string; stage: string }) => void = () => {};
  const seen: string[] = [];
  const result = followJob({ jobId: 'new', signal: new AbortController().signal,
    subscribe: async fn => { deliver = fn; return () => { seen.push('unsubscribed'); }; },
    start: async () => { deliver({ jobId: 'old', stage: 'ready' }); deliver({ jobId: 'new', stage: 'downloading' }); deliver({ jobId: 'new', stage: 'ready' }); },
    cancel: async () => {}, terminal: e => e.stage === 'ready', progress: e => seen.push(e.stage),
  });
  assert.deepEqual(await result, { jobId: 'new', stage: 'ready' });
  assert.deepEqual(seen, ['downloading', 'ready', 'unsubscribed']);
});

test('cancel during startup cancels only that job and discards later success', async () => {
  const abort = new AbortController(); const cancelled: string[] = []; let subscriptions = 0;
  const result = followJob({ jobId: 'active', signal: abort.signal,
    subscribe: async fn => { subscriptions++; return () => { subscriptions--; fn({ jobId: 'active', stage: 'ready' }); }; },
    start: async () => { abort.abort(); }, cancel: async id => { cancelled.push(id); },
    terminal: (e: {jobId:string;stage:string}) => e.stage === 'ready', progress: () => {},
  });
  await assert.rejects(result, /cancelled/);
  assert.ok(cancelled.length > 0 && cancelled.every(id => id === 'active')); assert.equal(subscriptions, 0);
});

test('failure to start unsubscribes and does not leave a pending operation', async () => {
  let cleaned = false;
  await assert.rejects(followJob({jobId:'a',signal:new AbortController().signal,
    subscribe: async () => () => { cleaned = true; }, start: async () => { throw Error('setup_busy'); },
    cancel: async () => {}, terminal: (_e: {jobId:string}) => false, progress: () => {},
  }), /setup_busy/);
  assert.equal(cleaned, true);
});

test('first analysis requires a tested AI and selected local mail, not a live Gmail connection', () => {
  assert.equal(canStartFirstAnalysis('saved_unverified', 3), false);
  assert.equal(canStartFirstAnalysis('ready', 3), true, 'downloaded mail remains usable after Gmail disconnects');
  assert.equal(canStartFirstAnalysis('ready', 0), false);
});
