import test from 'node:test';
import assert from 'node:assert/strict';
import { ADVANCED_SECTIONS, NAV_GROUPS, legacySectionFor, sectionLabel, type SettingsSectionId } from '../src/settings/sections.ts';
import { ROWS, ROWS_BY_ID, rowText } from '../src/settings/rows.ts';
import { searchRows } from '../src/settings/search.ts';

// Task 4: section registry, row metadata and search. All pure data and logic,
// so the dialog can search sections that are not mounted.

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

const ALL: SettingsSectionId[] = [...NAV_GROUPS.flat(), ...ADVANCED_SECTIONS];
const OLD_SECTIONS = [
  'account', 'ai', 'usage', 'keys', 'google', 'notion', 'github', 'obsidian',
  'voice', 'assistant', 'background', 'appearance', 'storage', 'about',
];

test('nav groups follow the design board and advanced has five pages', () => {
  assert.deepEqual(NAV_GROUPS, [
    ['general', 'ai', 'usage'],
    ['mail', 'briefing', 'voice'],
    ['connections', 'privacy', 'setup', 'about'],
  ]);
  assert.deepEqual(ADVANCED_SECTIONS, [
    'advanced-models', 'advanced-cloud', 'advanced-local', 'advanced-integrations', 'advanced-developer',
  ]);
});

test('every section has an English and a Traditional Chinese label', () => {
  for (const id of ALL) {
    assert.ok(sectionLabel(id, en).length > 0, id);
    assert.ok(sectionLabel(id, zh).length > 0, id);
  }
  assert.equal(sectionLabel('ai', en), 'Your AI');
  assert.equal(sectionLabel('ai', zh), '你的 AI');
  assert.equal(sectionLabel('mail', zh), '郵件與跟進');
  assert.equal(sectionLabel('advanced-cloud', zh), '雲端服務');
});

test('legacySectionFor returns an old Settings section id or null', () => {
  for (const id of ALL) {
    const legacy = legacySectionFor(id);
    assert.ok(legacy === null || OLD_SECTIONS.includes(legacy), `${id} -> ${legacy}`);
  }
  assert.equal(legacySectionFor('setup'), null);
  assert.equal(legacySectionFor('mail'), null);
  assert.equal(legacySectionFor('ai'), 'ai');
  assert.equal(legacySectionFor('privacy'), 'storage');
});

test('every row id is unique', () => {
  const ids = ROWS.map((row) => row.id);
  assert.equal(new Set(ids).size, ids.length);
});

test('every row belongs to a valid section', () => {
  for (const row of ROWS) assert.ok(ALL.includes(row.section), `${row.id}: ${row.section}`);
});

test('every section except setup has at least one row, and rows of a section stay together', () => {
  for (const id of ALL) assert.ok(ROWS.some((row) => row.section === id), id);
  const order = ROWS.map((row) => row.section).filter((s, i, all) => i === 0 || all[i - 1] !== s);
  assert.equal(new Set(order).size, order.length, 'rows of one section must be contiguous');
});

test('ROWS_BY_ID has one entry per row', () => {
  assert.equal(Object.keys(ROWS_BY_ID).length, ROWS.length);
  for (const row of ROWS) assert.equal(ROWS_BY_ID[row.id], row);
});

test('rowText returns the title and description for the locale', () => {
  const language = ROWS_BY_ID.language;
  assert.equal(rowText(language, 'en').title, 'Language');
  assert.equal(rowText(language, 'zh-HK').title, '語言');
  const withDescription = ROWS.find((row) => row.description);
  assert.ok(withDescription);
  assert.equal(rowText(withDescription, 'en').description, withDescription.description![0]);
  assert.equal(rowText(withDescription, 'zh-HK').description, withDescription.description![1]);
  const without = ROWS.find((row) => !row.description);
  assert.ok(without);
  assert.equal(rowText(without, 'en').description, undefined);
});

test('removed rows are not registered', () => {
  const titles = ROWS.map((row) => row.title[0].toLowerCase());
  assert.ok(!titles.includes('display name'));
  assert.ok(!ROWS.some((row) => row.id === 'display-name'));
});

test('zh-HK copy is written Chinese, never colloquial Cantonese', () => {
  for (const row of ROWS) {
    const text = [row.title[1], row.description?.[1] ?? ''].join('');
    assert.doesNotMatch(text, /[嘅咗唔係冇啲]/, row.id);
    assert.doesNotMatch(row.title.concat(row.description ?? []).join(''), /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/u, row.id);
  }
});

test('searching "lang" lists General first and finds the language row', () => {
  const result = searchRows('lang', ROWS, 'en');
  assert.equal(result[0].section, 'general');
  assert.ok(result[0].rows.some((row) => row.id === 'language'));
});

test('searching 語言 in zh-HK finds the language row', () => {
  const result = searchRows('語言', ROWS, 'zh-HK');
  assert.ok(result.flatMap((group) => group.rows).some((row) => row.id === 'language'));
});

test('matching ignores case and also searches the other language', () => {
  const upper = searchRows('LANG', ROWS, 'en').flatMap((g) => g.rows.map((r) => r.id));
  assert.ok(upper.includes('language'));
  const crossEn = searchRows('語言', ROWS, 'en').flatMap((g) => g.rows.map((r) => r.id));
  assert.ok(crossEn.includes('language'));
  const crossZh = searchRows('language', ROWS, 'zh-HK').flatMap((g) => g.rows.map((r) => r.id));
  assert.ok(crossZh.includes('language'));
});

test('an empty or blank query returns nothing', () => {
  assert.deepEqual(searchRows('', ROWS, 'en'), []);
  assert.deepEqual(searchRows('   ', ROWS, 'zh-HK'), []);
});

test('a query with no match returns nothing', () => {
  assert.deepEqual(searchRows('zzzzqqq', ROWS, 'en'), []);
});

test('sections come back in nav order and rows are not repeated', () => {
  const result = searchRows('a', ROWS, 'en');
  const positions = result.map((group) => ALL.indexOf(group.section));
  assert.deepEqual(positions, [...positions].sort((a, b) => a - b));
  for (const group of result) {
    assert.ok(group.rows.length > 0);
    assert.equal(new Set(group.rows.map((r) => r.id)).size, group.rows.length);
    for (const row of group.rows) assert.equal(row.section, group.section);
  }
});

test('title matches come before description and keyword matches inside a section', () => {
  const rows = [
    { id: 'a', section: 'general' as const, title: ['Alpha', '甲'] as [string, string], description: ['has needle here', '有針'] as [string, string] },
    { id: 'b', section: 'general' as const, title: ['Needle', '針'] as [string, string] },
    { id: 'c', section: 'general' as const, title: ['Gamma', '丙'] as [string, string], keywords: ['needle'] },
  ];
  const ids = searchRows('needle', rows, 'en')[0].rows.map((r) => r.id);
  assert.deepEqual(ids, ['b', 'a', 'c']);
});
