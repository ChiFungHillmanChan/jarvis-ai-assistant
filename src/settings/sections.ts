import type { Text } from '../lib/bedrock-models.ts';

// The settings dialog's sections: ids, nav groups and labels.

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
