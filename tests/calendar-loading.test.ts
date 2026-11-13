import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { document } from './dom.ts';
import { createElement, useEffect, useRef } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { eventsWhileLoading } from '../src/lib/calendar-layout.ts';

// R4-2. CalendarPage.tsx is JSX and imports CSS, which these tests cannot
// load, so what it draws while a range loads is the pure function
// eventsWhileLoading: tested here on its own, through the real hook, and in
// the page's source.

type Event = { id: number; start_time: string };
const a: Event = { id: 1, start_time: '2026-11-10T09:00:00Z' };
const b: Event = { id: 2, start_time: '2026-11-13T09:00:00Z' };

test('while a new range loads the last events stay; its reply replaces them, an empty one included; an error keeps nothing', () => {
  assert.deepEqual(eventsWhileLoading({ data: [a, b], error: null, loading: false }, null), [a, b], 'a reply is drawn');
  assert.deepEqual(eventsWhileLoading({ data: null, error: null, loading: true }, [a, b]), [a, b], 'loading: the last events stay');
  assert.deepEqual(eventsWhileLoading({ data: [b], error: null, loading: false }, [a, b]), [b], 'the new reply replaces them');
  assert.deepEqual(eventsWhileLoading({ data: [], error: null, loading: false }, [a, b]), [], 'an empty reply too');
  assert.equal(eventsWhileLoading({ data: null, error: 'database is locked', loading: false }, [a, b]), null, 'an error keeps nothing');
  assert.equal(eventsWhileLoading({ data: null, error: null, loading: true }, null), null, 'nothing drawn yet: nothing to keep');
  assert.deepEqual(eventsWhileLoading({ data: [a], error: 'refetch failed', loading: false }, [b]), [a], 'a refetch that fails keeps what the hook keeps');
});

// The same rule fed by the real hook, with the Tauri bridge replaced by
// replies the test sends: the page's two lines, here in a probe.
interface Pending { args: Record<string, unknown>; reply: (value: unknown) => void; fail: (error: unknown) => void }
const pending: Pending[] = [];
Object.assign(globalThis, {
  __TAURI_INTERNALS__: {
    invoke: (_command: string, args: Record<string, unknown>) => new Promise((reply, fail) => pending.push({ args, reply, fail })),
  },
});
const { useTauriCommand } = await import('../src/hooks/useTauriCommand.ts');
const settle = () => new Promise(resolve => setTimeout(resolve, 20));

test('through the hook: week to day keeps the week events until the day reply, then shows only that', async () => {
  const drawn: Array<{ range: string; events: Event[] | null }> = [];
  function Probe({ range }: { range: string }) {
    const visible = useTauriCommand<Event[]>('get_events_r4_2', { range });
    const lastVisible = useRef<Event[] | null>(null);
    const visibleEvents = eventsWhileLoading(visible, lastVisible.current);
    useEffect(() => { lastVisible.current = visibleEvents; });
    drawn.push({ range, events: visibleEvents });
    return null;
  }
  const root = createRoot(document.createElement('div') as unknown as HTMLElement);
  const render = (range: string) => flushSync(() => root.render(createElement(Probe, { range })));
  const last = () => drawn[drawn.length - 1];

  render('week');
  pending.shift()!.reply([a, b]);
  await settle();
  assert.deepEqual(last(), { range: 'week', events: [a, b] });

  render('day');
  assert.equal(pending.length, 1, 'the day is read');
  assert.ok(drawn.filter(d => d.range === 'day').every(d => d.events !== null && d.events.length === 2), 'the week events stay while the day loads');
  pending.shift()!.reply([]);
  await settle();
  assert.deepEqual(last(), { range: 'day', events: [] }, 'an empty day replaces them');

  render('next day');
  pending.shift()!.fail('database is locked');
  await settle();
  assert.deepEqual(last(), { range: 'next day', events: null }, 'an error keeps nothing');
  flushSync(() => root.unmount());
});

test('CalendarPage draws and selects from eventsWhileLoading, not from the bare read', () => {
  const page = readFileSync(new URL('../src/pages/CalendarPage.tsx', import.meta.url), 'utf8');
  assert.ok(page.includes('const visibleEvents = eventsWhileLoading(visible, lastVisible.current);'), 'the rule is applied');
  assert.ok(page.includes('useEffect(() => { lastVisible.current = visibleEvents; });'), 'what was drawn is kept');
  assert.ok(page.includes('toEntries(visibleEvents ?? [], mailItems)'), 'the grid draws it');
  assert.ok(page.includes('const allEvents = [...(visibleEvents ?? []), ...(todays.data ?? [])];'), 'the selected event is found in it');
  assert.ok(!page.includes('visible.data'), 'nothing reads the bare read');
});
