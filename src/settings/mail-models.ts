import type { MailSettings } from '../lib/mail.ts';

export type MailModelKey = 'local_model' | 'openai_model' | 'bedrock_model';

/**
 * The settings to save when one mail model changes: the copy just read from
 * the mail workspace with only that model replaced. mail_save_settings writes
 * every field, processing_mode included, and choose_ai can change the mode
 * while Advanced > Models is open, so the copy read when the page opened
 * would put an old mode back.
 */
export function mergeMailSetting(fresh: MailSettings, key: MailModelKey, value: string): MailSettings {
  return { ...fresh, [key]: value };
}
