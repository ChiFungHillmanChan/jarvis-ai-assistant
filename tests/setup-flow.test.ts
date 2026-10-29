import test from 'node:test';
import assert from 'node:assert/strict';
import { followJob, canStartFirstAnalysis, triageConsentText } from '../src/lib/setup-flow.ts';

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

const en = (value: string) => value;
const zh = (_en: string, value: string) => value;

test('the AI sorting consent names who receives the mail', () => {
  const chosenOnly = { typesafe_key: false, chosen_ai: 'OpenAI' };
  assert.match(triageConsentText(chosenOnly, en), /sent to OpenAI/);
  assert.doesNotMatch(triageConsentText(chosenOnly, en), /TypeSafe/);
  assert.match(triageConsentText(chosenOnly, zh), /OpenAI/);

  // With a TypeSafe key, Jev classifies every email and the chosen AI writes summaries.
  const withJev = { typesafe_key: true, chosen_ai: 'Claude' };
  assert.match(triageConsentText(withJev, en), /TypeSafe Jev/);
  assert.match(triageConsentText(withJev, en), /go to Claude/);
  assert.match(triageConsentText(withJev, zh), /TypeSafe Jev.*Claude/);

  // Ollama runs on this Mac: nothing leaves it.
  assert.match(triageConsentText({ typesafe_key: false, chosen_ai: 'Ollama' }, en), /does not leave this Mac/);
});

test('the AI sorting consent is written Chinese, not colloquial Cantonese', () => {
  for (const triage of [
    { typesafe_key: false, chosen_ai: 'OpenAI' },
    { typesafe_key: true, chosen_ai: 'Gemini' },
    { typesafe_key: true, chosen_ai: null },
    { typesafe_key: false, chosen_ai: 'Ollama' },
  ]) {
    assert.doesNotMatch(triageConsentText(triage, zh), /[係嘅咗唔啲嚟咁冇]/);
  }
});
