import test from 'node:test';
import assert from 'node:assert/strict';
import { ADVANCED_SECTIONS, NAV_GROUPS, sectionLabel, type SettingsSectionId } from '../src/settings/sections.ts';
import { ROWS, ROWS_BY_ID, rowText } from '../src/settings/rows.ts';
import { searchRows } from '../src/settings/search.ts';

// Task 4: section registry, row metadata and search. All pure data and logic,
// so the dialog can search sections that are not mounted.

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

const ALL: SettingsSectionId[] = [...NAV_GROUPS.flat(), ...ADVANCED_SECTIONS];

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

test('every row id is unique', () => {
  const ids = ROWS.map((row) => row.id);
  assert.equal(new Set(ids).size, ids.length);
});

test('every row belongs to a valid section', () => {
  for (const row of ROWS) assert.ok(ALL.includes(row.section), `${row.id}: ${row.section}`);
});

test('every section has at least one row, and rows of a section stay together', () => {
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

test('required rows exist', () => {
  const required = [
    'account', 'language', 'purge-cache', 'purge-backups', 'purge-models', 'purge-mail-bodies', 'purge-conversations',
    'delete-everything', 'ai-consents', 'chain-order', 'local-endpoints', 'key-gemini', 'key-claude', 'key-openai',
    'key-typesafe', 'aws-secret-key', 'google-other-accounts', 'keep-recordings',
  ];
  for (const id of required) assert.ok(ROWS_BY_ID[id], id);
});

test('disconnect and remove find the rows that have those actions', () => {
  const ids = (q: string) => searchRows(q, ROWS, 'en').flatMap((g) => g.rows.map((r) => r.id));
  assert.ok(ids('disconnect').includes('google-other-accounts'));
  assert.ok(ids('remove').includes('google-other-accounts'));
  assert.ok(ids('remove').includes('key-typesafe'));
  assert.ok(searchRows('移除', ROWS, 'zh-HK').flatMap((g) => g.rows.map((r) => r.id)).includes('key-typesafe'));
  assert.ok(searchRows('中斷連接', ROWS, 'zh-HK').flatMap((g) => g.rows.map((r) => r.id)).includes('google-other-accounts'));
});

test('zh-HK copy is written Chinese, never colloquial Cantonese', () => {
  const colloquial = /[嘅咗唔係冇啲喺嚟]/;
  const emoji = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/u;
  for (const row of ROWS) {
    const zhText = [row.title[1], row.description?.[1] ?? '', ...(row.keywords ?? [])].join('\n');
    assert.doesNotMatch(zhText, colloquial, row.id);
    assert.doesNotMatch([...row.title, ...(row.description ?? []), ...(row.keywords ?? [])].join(''), emoji, row.id);
  }
  for (const id of ALL) assert.doesNotMatch(sectionLabel(id, zh), colloquial, id);
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
