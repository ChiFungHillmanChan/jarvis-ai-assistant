import test from 'node:test';
import assert from 'node:assert/strict';
import { accountBadge } from '../src/lib/account.ts';

// The sidebar row has to render something sensible before the user has ever
// opened Settings, so every case here is about what the row shows when the
// stored display name is missing, blank, or not Latin text.

test('a saved display name becomes the row label and its initial', () => {
  const badge = accountBadge('Hillman Chan', 'Settings');
  assert.equal(badge.label, 'Hillman Chan');
  assert.equal(badge.initial, 'H');
  assert.equal(badge.isPlaceholder, false);
});

test('surrounding whitespace never reaches the label or the initial', () => {
  const badge = accountBadge('  Hillman Chan  ', 'Settings');
  assert.equal(badge.label, 'Hillman Chan');
  assert.equal(badge.initial, 'H');
});

test('a name that is unset or blank falls back to the navigation label', () => {
  for (const value of [null, undefined, '', '   ']) {
    const badge = accountBadge(value, '設定');
    assert.equal(badge.label, '設定', `for ${JSON.stringify(value)}`);
    assert.equal(badge.isPlaceholder, true, `for ${JSON.stringify(value)}`);
    // No initial: the row draws a settings icon instead of a letter circle.
    assert.equal(badge.initial, null, `for ${JSON.stringify(value)}`);
  }
});

test('the initial is upper-cased for Latin names and left alone otherwise', () => {
  assert.equal(accountBadge('hillman', 'Settings').initial, 'H');
  assert.equal(accountBadge('陳大文', '設定').initial, '陳');
});

test('the initial is a whole code point, not half a surrogate pair', () => {
  // charAt(0) would split the pair and render a replacement glyph.
  assert.equal(accountBadge('𝒥arvis', 'Settings').initial, '𝒥');
});
