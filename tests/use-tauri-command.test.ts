import test from 'node:test';
import assert from 'node:assert/strict';
import { document } from './dom.ts';
import { createElement } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';

// The hook is rendered for real, with the Tauri bridge replaced by replies
// the test sends when it chooses.
interface Pending { command: string; args: Record<string, unknown>; reply: (value: unknown) => void }
const pending: Pending[] = [];
Object.assign(globalThis, {
  __TAURI_INTERNALS__: {
    invoke: (command: string, args: Record<string, unknown>) => new Promise(reply => pending.push({ command, args, reply })),
  },
});
const { useTauriCommand } = await import('../src/hooks/useTauriCommand.ts');

const settle = () => new Promise(resolve => setTimeout(resolve, 20));

/** The one read on its way for this command, answered. */
function answer(command: string, value: unknown) {
  const at = pending.findIndex(read => read.command === command);
  assert.ok(at >= 0, `a read of ${command} is on its way`);
  pending.splice(at, 1)[0].reply(value);
}
const waiting = (command: string) => pending.filter(read => read.command === command).length;

// U3: the Usage panel's period selector. A reply for 90 days that arrives
// after the user went back to 30 must not be shown under 30, and while the
// reply for new arguments is on its way the old arguments' data is not shown.
test('a late reply for arguments no longer shown is ignored, and old data is not shown under new arguments', async () => {
  const command = 'get_cost_stats_u3';
  const shown: Array<{ days: number; data: unknown; loading: boolean }> = [];
  let refetch = () => {};
  function Probe({ days }: { days: number }) {
    const result = useTauriCommand<{ days: number; read?: number }>(command, { days });
    shown.push({ days, data: result.data, loading: result.loading });
    refetch = result.refetch;
    return null;
  }
  const container = document.createElement('div');
  const root = createRoot(container as unknown as HTMLElement);
  const render = (days: number) => flushSync(() => root.render(createElement(Probe, { days })));
  const last = () => shown[shown.length - 1];

  render(30);
  answer(command, { days: 30 });
  await settle();
  assert.deepEqual(last(), { days: 30, data: { days: 30 }, loading: false });

  render(90);
  assert.equal(waiting(command), 1, 'the 90-day read is sent');
  assert.ok(shown.filter(s => s.days === 90).every(s => s.data === null), '30 days of data are never shown under 90');
  assert.equal(last().loading, true);

  render(30);
  assert.deepEqual(last(), { days: 30, data: { days: 30 }, loading: false }, 'cached, shown at once');
  assert.equal(waiting(command), 1, 'and not read again');

  // The user asks for 30 days afresh; then the 90-day reply arrives late.
  flushSync(() => refetch());
  assert.equal(waiting(command), 2);
  const late = pending.find(read => read.command === command && read.args.days === 90)!;
  pending.splice(pending.indexOf(late), 1);
  late.reply({ days: 90 });
  await settle();
  assert.deepEqual(last(), { days: 30, data: { days: 30 }, loading: true }, 'the late 90-day reply stays off screen');
  answer(command, { days: 30, read: 2 });
  await settle();
  assert.deepEqual(last(), { days: 30, data: { days: 30, read: 2 }, loading: false });

  render(90);
  assert.deepEqual(last(), { days: 90, data: { days: 90 }, loading: false }, 'the late reply was cached for 90');
  flushSync(() => root.unmount());
});

// The other users of the hook pass fixed arguments: a refetch keeps what the
// same arguments showed until the new reply, as before.
test('a refetch with the same arguments keeps showing their data until the reply', async () => {
  let api: { data: unknown; refetch: () => void } | null = null;
  const seen: unknown[] = [];
  function Probe() {
    const result = useTauriCommand<number>('get_notion_stats_u3');
    api = result;
    seen.push(result.data);
    return null;
  }
  const root = createRoot(document.createElement('div') as unknown as HTMLElement);
  flushSync(() => root.render(createElement(Probe)));
  answer('get_notion_stats_u3', 1);
  await settle();
  assert.equal(api!.data, 1);
  const before = seen.length;
  flushSync(() => api!.refetch());
  assert.ok(seen.slice(before).every(value => value === 1), 'no gap while it refetches');
  answer('get_notion_stats_u3', 2);
  await settle();
  assert.equal(api!.data, 2);
  flushSync(() => root.unmount());
});
