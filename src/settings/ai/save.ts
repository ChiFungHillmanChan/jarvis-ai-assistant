// What the Your AI cards share when they save: the call they make, how a
// failure reads, and the cancelled confirmation, which is no error at all.
import { errorLine } from '../../lib/ai-error';
import type { AiChoice } from '../../lib/ai-choice';

type Text = (en: string, zh: string) => string;

/** Tests the choice and saves it (YourAi). Rejects with the reason; rejects with CANCELLED when the user kept the provider order. */
export type TestAndUse = (choice: AiChoice) => Promise<void>;

/** The user answered no to replacing the provider order: nothing was sent or saved. */
export const CANCELLED = new Error('cancelled');

/** The inline line for a failed save in the interface language; empty for CANCELLED. */
export function saveErrorLine(e: unknown, text: Text): string {
  return e === CANCELLED ? '' : errorLine(e, text);
}
