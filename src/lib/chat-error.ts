import type { Text } from './mail-presentation';
import { parseAiError, problemText } from './ai-error.ts';

/**
 * What a failed assistant turn means for the user, as a line of text. A
 * model failure or held-back email comes as a card (src/lib/ai-error.ts,
 * shown by AssistantPanel); this is for anything else, and for a card's
 * error shown where no card fits. Since P06 the router never tries another
 * model, so there is no list of providers to explain.
 */
export function chatErrorText(error: string, text: Text): { message: string; detail: string | null } {
  const problem = parseAiError(error);
  return { message: problem ? problemText(problem, text) : error, detail: null };
}
