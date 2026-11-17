import { catalog, interpolate, type MessageKey, type UiLocale } from '../i18n/catalog.ts';

/**
 * The two AI problems a page shows as a card instead of a line of text
 * (P06). A request the user waits on fails with one of them as its error,
 * a JSON envelope (src-tauri/src/ai/failure.rs Failure::to_error and
 * ai/consent.rs Request::to_error):
 *
 * - failure: the model did not answer, or cannot do what the request needs.
 *   The card names it, says why and offers the models that can, with keys.
 *   JARVIS never moves the request to another model on its own.
 * - consent: email and calendar content waits for the user's agreement to
 *   the provider's data terms (Gemini's billing, a service JARVIS cannot
 *   vouch for).
 */

/** A model as a chain row holds it (capability::Pick). */
export interface Pick { provider_type: string; base_url?: string | null; endpoint_id?: string | null; model_id?: string | null }
export interface AiTarget { pick: Pick; provider: string; model: string | null; label: string }
/** A model the card offers. `saves_at`: the replacement for a disabled model, saved at that chain position when picked. */
export interface Alternative { pick: Pick; label: string; saves_at: number | null }
export type FailureCode = 'no_key' | 'bad_key' | 'rate_limited' | 'bad_request' | 'not_found' | 'unreachable' | 'empty_reply' | 'server_error' | 'failed' | 'disabled' | 'unsupported' | 'no_model';
export interface Failure { feature: string; model: AiTarget; code: FailureCode; capability: string | null; message: string; alternatives: Alternative[]; actions_ran: number }
export type DataTerms = 'needs_billing_confirmation' | 'unknown';
export interface ConsentRequest { provider: string; base_url: string; label: string; terms: DataTerms; blocked_key: string; confirm_key: string }
export type AiProblem = { kind: 'failure'; failure: Failure } | { kind: 'consent'; consent: ConsentRequest };

/** The card an error carries, or null for any other error. */
export function parseAiError(error: unknown): AiProblem | null {
  const raw = typeof error === 'string' ? error : error instanceof Error ? error.message : String(error);
  if (!raw.startsWith('{')) return null;
  try {
    const value = (JSON.parse(raw) as { jarvis_ai?: { kind?: string; failure?: Failure; consent?: ConsentRequest } }).jarvis_ai;
    if (value?.kind === 'failure' && value.failure) return { kind: 'failure', failure: value.failure };
    if (value?.kind === 'consent' && value.consent) return { kind: 'consent', consent: value.consent };
  } catch { /* Not an envelope: shown as the error it is. */ }
  return null;
}

/** Google's page on turning billing on for a Gemini API key. */
export const GEMINI_BILLING_PAGE = 'https://aistudio.google.com/docs/billing';

const UNKNOWN_TERMS: [MessageKey, MessageKey] = ['consentUnknownTermsBlocked', 'consentUnknownTermsConfirm'];
const isKey = (key: string): key is MessageKey => Object.prototype.hasOwnProperty.call(catalog.en, key);

/**
 * The card's two sentences: what is held back and why, and what the button
 * agrees to. The keys are the registry's (`consent_copy_keys`); a key this
 * build does not have falls back to the "cannot vouch" card, never to
 * nothing.
 */
export function consentCopy(request: ConsentRequest, locale: UiLocale): { blocked: string; confirm: string; billingPage: string | null } {
  const known = isKey(request.blocked_key) && isKey(request.confirm_key);
  const [blocked, confirm] = known ? [request.blocked_key as MessageKey, request.confirm_key as MessageKey] : UNKNOWN_TERMS;
  const fill = (key: MessageKey) => interpolate(catalog[locale][key], { label: request.label });
  return { blocked: fill(blocked), confirm: fill(confirm), billingPage: request.terms === 'needs_billing_confirmation' && request.provider === 'gemini' ? GEMINI_BILLING_PAGE : null };
}

/**
 * What picking `choice` on the error card does. A replacement for a
 * disabled model is saved first (it is the only way to keep using that
 * row), then the request runs on the saved chain; any other pick runs
 * that one request on the picked model and changes no setting.
 */
export function pickAction(failure: Failure, choice: Alternative): { save: { place: { place: 'chain'; position: number }; model: string } | null; pick: Pick | null } {
  if (choice.saves_at !== null && failure.model.model) return { save: { place: { place: 'chain', position: choice.saves_at }, model: failure.model.model }, pick: null };
  return { save: null, pick: choice.pick };
}

/** The models the card offers. None once an action ran: the request is never sent again. */
export function offeredModels(failure: Failure): Alternative[] {
  return failure.actions_ran > 0 ? [] : failure.alternatives;
}

/** A consent as Settings lists it (ai::consent::Given). */
export interface GivenConsent { provider: string; base_url: string; label: string; terms: DataTerms; terms_now: DataTerms | 'safe'; registry_version: number; consented_at: string; holds: boolean }

/** When it was given, and whether it still holds (stricter terms since then ask again). */
export function consentStatus(consent: GivenConsent, text: (en: string, zh: string) => string): string {
  const day = consent.consented_at.slice(0, 10);
  if (!consent.holds) return text(`Agreed on ${day}. The terms are stricter now, so JARVIS asks again before sending.`, `於 ${day} 同意。條款其後收緊，JARVIS 會在傳送前再次詢問。`);
  return text(`Agreed on ${day}.`, `於 ${day} 同意。`);
}

/** A card's problem as one sentence, where no card fits (setup, a log line). */
export function problemText(problem: AiProblem, text: (en: string, zh: string) => string): string {
  return problem.kind === 'failure'
    ? problem.failure.message
    : text(`Email and calendar content waits for your agreement to ${problem.consent.label}'s data terms.`, `電郵及日曆內容須待你同意 ${problem.consent.label} 的資料條款後才會傳送。`);
}

/** One background notice per launch for each message (ai-needs-model, ai-consent-needed). */
export function addNotice<T>(notices: T[], notice: T, key: (n: T) => string): T[] {
  return notices.some(n => key(n) === key(notice)) ? notices : [...notices, notice];
}
