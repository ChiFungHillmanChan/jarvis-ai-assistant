import test from 'node:test';
import assert from 'node:assert/strict';
import { chatErrorText } from '../src/lib/chat-error.ts';

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

test('an empty Gemini reply names Gemini and suggests another model', () => {
  const error = 'All providers failed: gemini (gemini-3.6-flash): Gemini (gemini-3.6-flash) returned no text (finish_reason=STOP)';
  assert.equal(chatErrorText(error, en).message, 'Gemini returned an empty answer. Try again, or choose another model in Settings → Your AI.');
  assert.equal(chatErrorText(error, zh).message, 'Gemini 沒有回覆任何內容。請再試一次，或到「設定 → 你的 AI」改用其他模型。');
  assert.equal(chatErrorText(error, en).detail, error);
});

test('an empty reply from another provider keeps the general message', () => {
  const error = 'All providers failed: gemini (gemini-3.6-flash): Gemini API error 500; Claude (claude-sonnet-4-6) returned no text';
  assert.equal(chatErrorText(error, en).message, 'Your AI could not answer just now. Try again shortly.');
});

test('an empty Gemini reply after a completed action says the action was kept', () => {
  const error = 'Stopped after 1 completed action(s) so they are not repeated: gemini (gemini-3.7-flash): Gemini (gemini-3.7-flash) returned no text (finish_reason=STOP)';
  assert.equal(chatErrorText(error, en).message, 'Gemini returned an empty answer. The actions already completed were kept and will not be repeated. Check the result, or choose another model in Settings → Your AI.');
  assert.equal(chatErrorText(error, zh).message, 'Gemini 沒有回覆任何內容。已完成的操作已保留，不會重複執行。請檢查結果，或到「設定 → 你的 AI」改用其他模型。');
  assert.equal(chatErrorText(error, en).detail, error);
});

test('a later provider that fails after an action is not reported as an empty Gemini reply', () => {
  // Gemini returned nothing and ran no tool, so the router moved on; Claude
  // ran a tool and then failed. The router's string holds both errors.
  const error = 'Stopped after 1 completed action(s) so they are not repeated: gemini (gemini-3.7-flash): Gemini (gemini-3.7-flash) returned no text (finish_reason=STOP); claude (claude-sonnet-4-6): Claude API error 500';
  assert.deepEqual(chatErrorText(error, en), { message: error, detail: null });
  assert.deepEqual(chatErrorText(error, zh), { message: error, detail: null });
  // The other way round, Gemini's empty reply is the last error and decides.
  const geminiLast = 'Stopped after 1 completed action(s) so they are not repeated: claude (claude-sonnet-4-6): Claude API error 500; gemini (gemini-3.7-flash): Gemini (gemini-3.7-flash) returned no text (finish_reason=STOP)';
  assert.match(chatErrorText(geminiLast, en).message, /^Gemini returned an empty answer\. The actions already completed were kept/);
});

test('another failure after a completed action is shown as the router wrote it', () => {
  const error = 'Stopped after 2 completed action(s) so they are not repeated: claude (claude-sonnet-4-6): Claude API error 500';
  assert.deepEqual(chatErrorText(error, en), { message: error, detail: null });
});
