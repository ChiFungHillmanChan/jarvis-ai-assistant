import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { addNotice, consentCopy, consentStatus, errorLine, noticeHeading, offeredModels, offersPlainResend, parseAiError, pickAction, pickerText, problemText, serviceSaid, GEMINI_BILLING_PAGE, type ConsentRequest, type Failure, type GivenConsent } from '../src/lib/ai-error.ts';
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
  assert.match(billing.blocked, /^Email, calendar content and voice recordings are not sent to Google Gemini yet\. Gemini’s free tier/);
  assert.equal(billing.confirm, 'My Gemini API key has billing set up. Send email, calendar content and voice to Google Gemini');
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

test('a refused Think harder turn can be sent again without it, unless an action ran', () => {
  assert.equal(offersPlainResend(failure({ feature: 'premium', provider_reason: 'Thinking level is not supported for this model.' })), true);
  assert.equal(offersPlainResend(failure({ feature: 'premium', actions_ran: 1 })), false);
  assert.equal(offersPlainResend(failure({ feature: 'chat' })), false);
});

// Google Limited Use (DECISIONS, P06, 2026-11-17): Gmail and Calendar
// content is not sent to a service JARVIS cannot confirm. The request ends
// on the card, which offers the models that may receive the content.
test('a request refused Google content offers the models that may receive it', () => {
  const card = failure({
    code: 'google_content',
    message: 'Gmail and Calendar content is not sent to Kimi: JARVIS cannot confirm how this service uses data. Choose an AI whose terms JARVIS can confirm for this.',
  });
  const parsed = parseAiError(JSON.stringify({ jarvis_ai: { kind: 'failure', failure: card } }));
  assert.equal(parsed?.kind, 'failure');
  assert.equal(problemText(parsed!, en), card.message);
  assert.deepEqual(pickerText(card, en), {
    intro: 'Answer this with an AI that may receive Gmail and Calendar content (your saved choice stays as it is):',
    none: 'No AI with a saved key may receive Gmail and Calendar content. Choose one in Settings → Your AI.',
  });
  assert.equal(pickerText(card, zh).intro, '改用可接收 Gmail 及日曆內容的 AI 回答這個請求（已儲存的選擇不會更改）：');
  assert.equal(pickerText(failure(), en).intro, 'Run this request on another model (your saved choice stays as it is):');
  assert.deepEqual(offeredModels({ ...card, actions_ran: 1 }), [], 'never sent again after an action ran');
});

test('a background notice for content not sent says so in its heading', () => {
  assert.equal(noticeHeading('google_content', en), 'Gmail and Calendar content stays back');
  assert.equal(noticeHeading('google_content', zh), 'Gmail 及日曆內容未有傳送');
  assert.equal(noticeHeading('unsupported', en), 'A job could not run on your AI');
  assert.equal(noticeHeading(undefined, zh), '一項背景工作未能使用你的 AI 執行');
});

// F2 (native-qa, 2026-11-19): "Test and use this AI" in Settings → Your AI
// and in the setup guide showed the failure card's raw data
// ({"jarvis_ai":{"failure":...). The panel has no card, so the line is the
// card's sentence, then the service's own reason as the card gives it.
test("a failed test shows the card's sentence and the service's reason, never its data", () => {
  const scoped = 'This API key is not scoped to a workspace, so this request must include the anthropic-workspace-id header';
  const refused = (message: string) => JSON.stringify({ jarvis_ai: { kind: 'failure', failure: failure({ message, provider_reason: scoped }) } });
  assert.equal(
    errorLine(refused('Claude Sonnet 5.5 (Anthropic Claude) did not accept this request (400). Check the chosen model, or pick another one.'), en),
    `Claude Sonnet 5.5 (Anthropic Claude) did not accept this request (400). Check the chosen model, or pick another one. The service said: ${scoped}`,
  );
  assert.equal(
    errorLine(refused('Claude Sonnet 5.5 (Anthropic Claude) 不接受這個請求（400），請檢查所選模型，或改用其他模型。'), zh),
    `Claude Sonnet 5.5 (Anthropic Claude) 不接受這個請求（400），請檢查所選模型，或改用其他模型。服務回覆：${scoped}`,
  );
  // As seen in the window: a 404 with no reason of the service's own.
  const notFound = 'Gemini 2.5 Flash (Google Gemini) could not find the chosen model or address (404).';
  const seen = JSON.stringify({ jarvis_ai: { kind: 'failure', failure: failure({ code: 'not_found', message: notFound, provider_reason: null }) } });
  assert.equal(errorLine(seen, en), notFound);
  assert.equal(errorLine(new Error(seen), en), notFound);
  // The test path cannot hold email back for consent (Feature::Other), but a
  // consent card would be a sentence too.
  assert.equal(errorLine(JSON.stringify({ jarvis_ai: { kind: 'consent', consent: kimi } }), zh), '電郵及日曆內容須待你同意 Kimi 的資料條款後才會傳送。');
  for (const shown of [errorLine(refused('x'), en), errorLine(seen, zh)]) assert.ok(!shown.includes('{') && !shown.includes('jarvis_ai'), shown);
  // Any other error, such as the model list's own, is shown as it is.
  assert.equal(errorLine('Claude rejected this API key. Check it and try again.', en), 'Claude rejected this API key. Check it and try again.');
  assert.equal(serviceSaid(scoped, zh), `服務回覆：${scoped}`);
});

test('Settings → Your AI and the setup guide show the error through that line', () => {
  const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
  // Settings → Your AI: each card's failed save reads through saveErrorLine,
  // which is errorLine (or nothing for the kept provider order).
  assert.match(source('../src/settings/ai/save.ts'), /return e === CANCELLED \? '' : errorLine\(e, text\);/);
  for (const card of ['GeminiCard', 'LocalCard', 'OtherServiceCard']) {
    const code = source(`../src/settings/ai/${card}.tsx`);
    assert.match(code, /saveErrorLine\(e, text\)/, card);
    assert.doesNotMatch(code, /setError\(String\(/, card);
  }
  const chooseAi = source('../src/components/ChooseAi.tsx');
  assert.match(chooseAi, /catch \(e\) \{ if \(mounted\.current\) setError\(errorLine\(e, text\)\); \}/);
  assert.doesNotMatch(chooseAi, /setError\(String\(/);
  // The setup guide's AI step is ChooseAi, so it shows the same line.
  assert.match(source('../src/pages/QuickSetup.tsx'), /<ChooseAi /);
  // The card and the line say "The service said" with the same words.
  assert.match(source('../src/components/ModelErrorCard.tsx'), /\{serviceSaid\(failure\.provider_reason, text\)\}/);
});
