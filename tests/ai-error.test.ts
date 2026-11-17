import test from 'node:test';
import assert from 'node:assert/strict';
import { addNotice, consentCopy, consentStatus, offeredModels, parseAiError, pickAction, GEMINI_BILLING_PAGE, type ConsentRequest, type Failure, type GivenConsent } from '../src/lib/ai-error.ts';
import { chatStatusText } from '../src/lib/chat-status.ts';

// P06: a request that fails, or email held back by a provider's data
// terms, comes to the page as a card. These tests pin what the cards say
// and what their buttons do, from the envelopes ai/failure.rs and
// ai/consent.rs send.

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;

const failure = (over: Partial<Failure> = {}): Failure => ({
  feature: 'chat',
  model: { pick: { provider_type: 'openai', base_url: null, endpoint_id: null, model_id: 'old-model' }, provider: 'openai', model: 'old-model', label: 'Old Model (OpenAI (GPT))' },
  code: 'bad_request',
  capability: null,
  message: 'Old Model (OpenAI (GPT)) did not accept this request.',
  alternatives: [
    { pick: { provider_type: 'claude', base_url: null, endpoint_id: null, model_id: null }, label: 'Claude (Anthropic Claude)', saves_at: null },
  ],
  actions_ran: 0,
  ...over,
});

const gemini: ConsentRequest = { provider: 'gemini', base_url: '', label: 'Google Gemini', terms: 'needs_billing_confirmation', blocked_key: 'consentGeminiBillingBlocked', confirm_key: 'consentGeminiBillingConfirm' };
const kimi: ConsentRequest = { provider: 'kimi', base_url: 'https://api.moonshot.ai/v1', label: 'Kimi', terms: 'unknown', blocked_key: 'consentUnknownTermsBlocked', confirm_key: 'consentUnknownTermsConfirm' };

test('the envelopes the backend sends become cards; anything else stays an error line', () => {
  assert.deepEqual(parseAiError(JSON.stringify({ jarvis_ai: { kind: 'failure', failure: failure() } })), { kind: 'failure', failure: failure() });
  assert.deepEqual(parseAiError(new Error(JSON.stringify({ jarvis_ai: { kind: 'consent', consent: kimi } }))), { kind: 'consent', consent: kimi });
  assert.equal(parseAiError('Gemini API error 400'), null);
  assert.equal(parseAiError('{"other": 1}'), null);
  assert.equal(parseAiError('{broken'), null);
});

test("the consent card uses the registry's copy keys and names the service", () => {
  const billing = consentCopy(gemini, 'en');
  assert.match(billing.blocked, /^Email and calendar content is not sent to Google Gemini yet\. Gemini’s free tier/);
  assert.equal(billing.confirm, 'My Gemini API key has billing set up. Send email and calendar content to Google Gemini');
  assert.equal(billing.billingPage, GEMINI_BILLING_PAGE);
  const unknown = consentCopy(kimi, 'zh-HK');
  assert.equal(unknown.blocked, '電郵及日曆內容暫未傳送至 Kimi。JARVIS 無法確認 Kimi 如何處理收到的內容，對方可能會保留內容或用作訓練模型。請在接受這一點後才傳送。');
  assert.equal(unknown.confirm, '我接受，可傳送電郵及日曆內容至 Kimi');
  assert.equal(unknown.billingPage, null);
});

test('a copy key this build does not have still shows the "cannot vouch" card', () => {
  const copy = consentCopy({ ...kimi, label: 'New AI', blocked_key: 'consentNewProviderBlocked', confirm_key: 'consentNewProviderConfirm' }, 'en');
  assert.match(copy.blocked, /^Email and calendar content is not sent to New AI yet\. JARVIS cannot vouch/);
  assert.equal(copy.confirm, 'I accept this. Send email and calendar content to New AI');
});

test('a pick on the error card runs that request on the picked model and saves nothing', () => {
  const card = failure();
  assert.deepEqual(pickAction(card, card.alternatives[0]), { save: null, pick: card.alternatives[0].pick });
});

test("a disabled model's replacement is saved at its chain position, then the request runs on the saved choice", () => {
  const replacement = { pick: { provider_type: 'openai', model_id: 'new-model' }, label: 'New Model (OpenAI (GPT))', saves_at: 3 };
  const card = failure({ code: 'disabled', alternatives: [replacement] });
  assert.deepEqual(pickAction(card, replacement), { save: { place: { place: 'chain', position: 3 }, model: 'old-model' }, pick: null });
});

test('after an action ran no model is offered, so the request cannot be sent again', () => {
  assert.equal(offeredModels(failure()).length, 1);
  assert.deepEqual(offeredModels(failure({ actions_ran: 1 })), []);
});

test('each background notice is shown once', () => {
  const key = (n: { message: string }) => n.message;
  const once = addNotice([], { message: 'Needs a model that supports mail sorting: X does not.' }, key);
  assert.equal(addNotice(once, { message: 'Needs a model that supports mail sorting: X does not.' }, key).length, 1);
  assert.equal(addNotice(once, { message: 'other' }, key).length, 2);
});

test('Settings says when a consent was given and when the terms changed since', () => {
  const given: GivenConsent = { provider: 'gemini', base_url: '', label: 'Google Gemini', terms: 'needs_billing_confirmation', terms_now: 'needs_billing_confirmation', registry_version: 1, consented_at: '2026-11-17 09:30:00', holds: true };
  assert.equal(consentStatus(given, en), 'Agreed on 2026-11-17.');
  assert.equal(consentStatus({ ...given, terms_now: 'unknown', holds: false }, zh), '於 2026-11-17 同意。條款其後收緊，JARVIS 會在傳送前再次詢問。');
});

test('the one resend to the same model is shown as such, never as a switch', () => {
  assert.equal(chatStatusText('Trying the same model again...', zh), '正在重試同一個模型…');
  assert.equal(chatStatusText('Trying the same model again...', en), 'Trying the same model again…');
});
