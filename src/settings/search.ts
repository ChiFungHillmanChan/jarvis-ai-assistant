import { ADVANCED_SECTIONS, NAV_GROUPS, type SettingsSectionId } from './sections.ts';
import type { RowMeta } from './rows.ts';

const NAV_ORDER: SettingsSectionId[] = [...NAV_GROUPS.flat(), ...ADVANCED_SECTIONS];

// 0 = title (either language), 1 = description, 2 = keyword, -1 = no match.
function rank(row: RowMeta, needle: string): number {
  const has = (text: string) => text.toLowerCase().includes(needle);
  if (row.title.some(has)) return 0;
  if (row.description?.some(has)) return 1;
  if (row.keywords?.some(has)) return 2;
  return -1;
}

// Case-insensitive match over both languages. Sections come back in nav order;
// inside a section, title matches come first, then description, then keywords,
// each in display order. `locale` is accepted so callers pass the active
// language, but matching deliberately covers both.
export function searchRows(
  query: string,
  rows: RowMeta[],
  _locale: 'en' | 'zh-HK',
): { section: SettingsSectionId; rows: RowMeta[] }[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const groups: { section: SettingsSectionId; rows: RowMeta[] }[] = [];
  for (const section of NAV_ORDER) {
    const matches = rows
      .map((row, index) => ({ row, index, rank: row.section === section ? rank(row, needle) : -1 }))
      .filter((m) => m.rank >= 0)
      .sort((a, b) => a.rank - b.rank || a.index - b.index)
      .map((m) => m.row);
    if (matches.length > 0) groups.push({ section, rows: matches });
  }
  return groups;
}
