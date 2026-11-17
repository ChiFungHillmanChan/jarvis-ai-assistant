import test from 'node:test';
import assert from 'node:assert/strict';
import { chatErrorText } from '../src/lib/chat-error.ts';

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

// P06: the router no longer walks a chain of providers, so a failed turn is
// an error card (src/lib/ai-error.ts) and this line is what remains of it.

test("a model failure is told in the card's own words, naming the model", () => {
  const failure = { feature: 'chat', model: { pick: { provider_type: 'gemini' }, provider: 'gemini', model: 'm', label: 'Gemini Test (Google Gemini)' }, code: 'empty_reply', capability: null, message: 'Gemini Test (Google Gemini) returned an empty answer. Try again, or pick another model.', alternatives: [], actions_ran: 0 };
  const error = JSON.stringify({ jarvis_ai: { kind: 'failure', failure } });
  assert.deepEqual(chatErrorText(error, en), { message: failure.message, detail: null });
});

test('held-back email names the service whose terms wait for agreement', () => {
  const consent = { provider: 'kimi', base_url: 'https://api.moonshot.ai/v1', label: 'Kimi', terms: 'unknown', blocked_key: 'consentUnknownTermsBlocked', confirm_key: 'consentUnknownTermsConfirm' };
  const error = JSON.stringify({ jarvis_ai: { kind: 'consent', consent } });
  assert.equal(chatErrorText(error, en).message, "Email and calendar content waits for your agreement to Kimi's data terms.");
  assert.equal(chatErrorText(error, zh).message, '電郵及日曆內容須待你同意 Kimi 的資料條款後才會傳送。');
});

test('any other error is shown as the backend wrote it', () => {
  const error = 'This request already carried out an action, such as creating an event or a draft. JARVIS does not send it again, so the action is not done twice.';
  assert.deepEqual(chatErrorText(error, en), { message: error, detail: null });
  assert.deepEqual(chatErrorText('{not json', en), { message: '{not json', detail: null });
});
