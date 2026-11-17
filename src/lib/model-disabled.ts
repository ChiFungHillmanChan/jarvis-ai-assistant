/**
 * The notice for a saved model the model registry disabled
 * (src-tauri/src/ai/disabled.rs). JARVIS never switches it on its own: the
 * notice names the replacement and saves it only when the user confirms.
 */

/** Where the disabled model is saved, as disabled.rs serialises Place. */
export type DisabledPlace = { place: 'chain'; position: number } | { place: 'mail'; field: 'local_model' | 'openai_model' | 'bedrock_model' };
/** One saved choice (disabled_models). */
export type DisabledChoice = DisabledPlace & { provider: string; model: string; replacement: string | null; replacement_label: string | null };
/** The model-disabled event: a request on this model stopped. */
export interface DisabledEvent { provider: string; model: string; replacement: string | null }

/** The notice is shown once on its own for each saved choice and replacement. */
export function choiceKey(choice: DisabledChoice): string {
  const place = choice.place === 'chain' ? `chain:${choice.position}` : `mail:${choice.field}`;
  return `${place}|${choice.provider}|${choice.model}|${choice.replacement ?? ''}`;
}

export function placeOf(choice: DisabledChoice): DisabledPlace {
  return choice.place === 'chain' ? { place: 'chain', position: choice.position } : { place: 'mail', field: choice.field };
}

/**
 * The notices to show now. `choices` is what disabled_models returned, so
 * anything already replaced drops out. A notice stays while it is open; one
 * the user has seen comes back only when a request on its model just
 * stopped (`event`).
 */
export function noticesToShow(open: DisabledChoice[], choices: DisabledChoice[], seen: ReadonlySet<string>, event: DisabledEvent | null): DisabledChoice[] {
  const openKeys = new Set(open.map(choiceKey));
  return choices.filter(choice => openKeys.has(choiceKey(choice))
    || !seen.has(choiceKey(choice))
    || (event !== null && event.provider === choice.provider && event.model === choice.model));
}

/** The notice text: what stopped, that nothing switches on its own, and the suggestion. */
export function noticeText(choice: DisabledChoice, text: (en: string, zh: string) => string): string {
  const where = choice.place === 'chain' ? text('provider priority', '供應商優先次序') : text('mail analysis', '電郵分析');
  const stops = text(
    `${choice.model} (${where}) is no longer available. JARVIS does not switch models on its own: until you choose, requests that use it stop and background jobs that use it do not run.`,
    `模型 ${choice.model}（${where}）已停用。JARVIS 不會自行更換模型：在你選擇前，使用此模型的請求會停止，使用此模型的背景工作亦不會執行。`,
  );
  if (!choice.replacement) return `${stops} ${text('Choose another model in Settings or on the Mail page.', '請在設定或電郵頁面選擇其他模型。')}`;
  const name = choice.replacement_label ? `${choice.replacement_label} (${choice.replacement})` : choice.replacement;
  const zhName = choice.replacement_label ? `${choice.replacement_label}（${choice.replacement}）` : choice.replacement;
  return `${stops} ${text(`Suggested replacement: ${name}.`, `建議替代模型：${zhName}。`)}`;
}
