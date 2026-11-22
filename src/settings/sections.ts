import type { Text } from '../lib/bedrock-models.ts';

// The settings dialog's sections: ids, nav groups, labels, and the old
// Settings.tsx section each one replaces (used by the bridge until the old
// page is deleted).

export type SettingsSectionId =
  | 'general' | 'ai' | 'usage' | 'mail' | 'briefing' | 'voice' | 'connections' | 'privacy' | 'setup' | 'about'
  | 'advanced-models' | 'advanced-cloud' | 'advanced-local' | 'advanced-integrations' | 'advanced-developer';

export const NAV_GROUPS: SettingsSectionId[][] = [
  ['general', 'ai', 'usage'],
  ['mail', 'briefing', 'voice'],
  ['connections', 'privacy', 'setup', 'about'],
];

export const ADVANCED_SECTIONS: SettingsSectionId[] = [
  'advanced-models', 'advanced-cloud', 'advanced-local', 'advanced-integrations', 'advanced-developer',
];

export function sectionLabel(id: SettingsSectionId, text: Text): string {
  switch (id) {
    case 'general': return text('General', '一般');
    case 'ai': return text('Your AI', '你的 AI');
    case 'usage': return text('Usage', '用量');
    case 'mail': return text('Mail and follow-ups', '郵件與跟進');
    case 'briefing': return text('Daily briefing', '每日簡報');
    case 'voice': return text('Voice', '語音');
    case 'connections': return text('Connections', '連接');
    case 'privacy': return text('Privacy and data', '私隱與資料');
    case 'setup': return text('Setup guide', '設定導引');
    case 'about': return text('About', '關於');
    case 'advanced-models': return text('Models', '模型');
    case 'advanced-cloud': return text('Cloud providers', '雲端服務');
    case 'advanced-local': return text('Local AI', '本機 AI');
    case 'advanced-integrations': return text('Integrations', '整合');
    case 'advanced-developer': return text('Developer', '開發者');
  }
}

export function advancedGroupLabel(text: Text): string {
  return text('Advanced (for developers)', '進階（開發者用）');
}

// Exists only for the temporary bridge (Task 5), which renders the old
// Settings.tsx body for a section that is not rebuilt yet. Deleted in Task 19.
// Returns every old section id whose body holds this section's rows, in display
// order, or [] when there is none (Mail lived in EmailPage; Setup guide is new).
// Never 'account': old Settings redirects it to 'ai' when accounts are
// unavailable, which is the case in release builds.
export function legacySectionsFor(id: SettingsSectionId): string[] {
  switch (id) {
    case 'general': return ['assistant', 'appearance', 'background'];
    case 'ai': return ['ai'];
    case 'usage': return ['usage'];
    case 'mail': return [];
    case 'briefing': return ['background'];
    case 'voice': return ['voice'];
    case 'connections': return ['google', 'notion', 'github', 'obsidian'];
    case 'privacy': return ['storage'];
    case 'setup': return [];
    case 'about': return ['about'];
    case 'advanced-models': return ['ai'];
    case 'advanced-cloud': return ['keys', 'ai'];
    case 'advanced-local': return ['ai'];
    case 'advanced-integrations': return ['notion', 'github', 'obsidian'];
    case 'advanced-developer': return ['ai', 'voice'];
  }
}
