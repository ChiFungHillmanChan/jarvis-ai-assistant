import test from 'node:test';
import assert from 'node:assert/strict';
import { setupError } from '../src/lib/setup.ts';

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

// P06, review B I-1: with no saved chain the mail check refuses when it
// cannot tell which AI would read the mail. The setup page says so in a
// sentence of its own, not the generic "did not finish" line.
test('the mail check that cannot tell which AI reads mail has its own sentence', () => {
  const generic = setupError('no_such_code', en);
  for (const code of ['mail_ai_undecided', 'mail_ai_not_allowed', 'google_content_not_sent', 'consent_needed']) {
    assert.notEqual(setupError(code, en), generic, code);
    assert.notEqual(setupError(code, zh), setupError('no_such_code', zh), code);
  }
  assert.equal(setupError('mail_ai_undecided', en), 'JARVIS could not tell which AI would read your mail, so nothing was sent. Choose an AI in Settings, or add its key, then test again.');
  assert.equal(setupError('mail_ai_undecided', zh), 'JARVIS 未能確定由哪個 AI 處理電郵，因此沒有傳送任何內容。請在設定中選擇 AI 或加入其密鑰，然後再測試。');
});
