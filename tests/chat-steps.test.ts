import test from 'node:test';
import assert from 'node:assert/strict';
import { chatStatusText } from '../src/lib/chat-status.ts';
import { addStep } from '../src/workspace/chatState.ts';
import type { ChatStep } from '../src/lib/types.ts';

const en = (value: string) => value;
const zh = (_: string, value: string) => value;

test('a running step says what it is looking for', () => {
  assert.equal(chatStatusText('Running: Searching emails...', zh, 'Gold Badge 截止'), '正在搜尋郵件：Gold Badge 截止');
  assert.equal(chatStatusText('Running: Searching emails...', en, 'Gold Badge'), 'Searching emails: Gold Badge');
});

test('a step reads as a plain action, not a log line', () => {
  assert.equal(chatStatusText('Running: Checking calendar...', en), 'Checking calendar…');
  assert.equal(chatStatusText('Planning: Checking calendar...', zh), '正在查看日曆…');
  assert.equal(chatStatusText('Composing response...', zh), '正在撰寫回覆…');
  assert.equal(chatStatusText('Something new...', zh), 'Something new…');
});

test('only running steps are kept, once each, in order', () => {
  let steps: ChatStep[] = [];
  for (const payload of [
    { status: 'Understanding your request...' },
    { status: 'Planning: Searching emails...' },
    { status: 'Running: Searching emails...', detail: 'Gold Badge' },
    { status: 'Running: Searching emails...', detail: 'Gold Badge' },
    { status: 'Reviewing tool results...' },
    { status: 'Running: Searching emails...', detail: 'Gold Badge deadline' },
    { status: 'Running: Checking follow-ups...' },
  ]) steps = addStep(steps, payload);
  assert.deepEqual(steps, [
    { status: 'Running: Searching emails...', detail: 'Gold Badge' },
    { status: 'Running: Searching emails...', detail: 'Gold Badge deadline' },
    { status: 'Running: Checking follow-ups...', detail: null },
  ]);
});
