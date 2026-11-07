import test from 'node:test';
import assert from 'node:assert/strict';
import { dateFormat } from '../src/lib/date-format.ts';

test('the same locale and options reuse one formatter', () => {
  assert.equal(dateFormat('en-GB', { weekday: 'short' }), dateFormat('en-GB', { weekday: 'short' }));
  assert.notEqual(dateFormat('en-GB', { weekday: 'short' }), dateFormat('zh-HK', { weekday: 'short' }));
  assert.notEqual(dateFormat('en-GB', { weekday: 'short' }), dateFormat('en-GB', { weekday: 'long' }));
});

test('a cached formatter writes what toLocaleDateString and toLocaleTimeString write', () => {
  const day = new Date(2026, 8, 27, 15, 5);
  for (const locale of ['en-GB', 'zh-HK']) {
    assert.equal(dateFormat(locale, { weekday: 'short' }).format(day), day.toLocaleDateString(locale, { weekday: 'short' }));
    assert.equal(dateFormat(locale, { weekday: 'long', month: 'long', day: 'numeric' }).format(day), day.toLocaleDateString(locale, { weekday: 'long', month: 'long', day: 'numeric' }));
    assert.equal(dateFormat(locale, { hour: 'numeric', minute: '2-digit' }).format(day), day.toLocaleTimeString(locale, { hour: 'numeric', minute: '2-digit' }));
  }
});
