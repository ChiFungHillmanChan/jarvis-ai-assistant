import { getVersion } from '@tauri-apps/api/app';
import { openUrl } from './commands';

export const SUPPORT_EMAIL = 'contact@jarvis-automation.com';

type Text = (english: string, traditionalChinese: string) => string;

/**
 * Opens a pre-filled email to the team in the user's mail app. Nothing is sent
 * from JARVIS: the user reads, edits and sends it, and can attach a screenshot.
 */
export async function reportProblem(text: Text): Promise<void> {
  const version = await getVersion().catch(() => 'unknown');
  const subject = text(`JARVIS problem report (v${version})`, `JARVIS 問題回報（v${version}）`);
  const body = text(
    'What were you trying to do?\n\n\nWhat happened instead?\n\n\nA screenshot helps if you can attach one.\n\n',
    '你當時想做甚麼？\n\n\n實際出現了甚麼情況？\n\n\n如可以，請附上截圖。\n\n',
  ) + `---\nJARVIS ${version}`;
  await openUrl(`mailto:${SUPPORT_EMAIL}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`);
}
