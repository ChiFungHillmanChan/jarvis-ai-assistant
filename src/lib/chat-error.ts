import type { Text } from './mail-presentation';

/**
 * What a failed assistant turn means for the user. The router's own text
 * ("All providers failed: Gemini (...): Gemini API error 400 ...") stays
 * available as technical detail.
 */
export function chatErrorText(error: string, text: Text): { message: string; detail: string | null } {
  const lower = error.toLowerCase();
  // ai/gemini.rs empty_reply_error: "Gemini (<model>) returned no text (...)".
  const emptyGemini = (part: string) => /gemini \([^)]*\) returned no text/.test(part);
  const geminiEmpty = emptyGemini(lower);
  // After a tool with side effects has run, the router stops instead of
  // falling back ("Stopped after N completed action(s) so they are not
  // repeated: ..."), so what already ran is kept. The router joins each
  // provider's error with "; ", in the order tried: only the last one
  // stopped the turn, so an empty Gemini reply earlier in the chain is not
  // blamed for another provider's failure.
  if (error.startsWith('Stopped after') && emptyGemini(lower.split('; ').pop() ?? '')) {
    return {
      message: text('Gemini returned an empty answer. The actions already completed were kept and will not be repeated. Check the result, or choose another model in Settings → Your AI.', 'Gemini 沒有回覆任何內容。已完成的操作已保留，不會重複執行。請檢查結果，或到「設定 → 你的 AI」改用其他模型。'),
      detail: error,
    };
  }
  if (!error.startsWith('All providers failed')) return { message: error, detail: null };
  const has = (...parts: string[]) => parts.some(part => lower.includes(part));
  const message = has('api error 401', 'api error 403', 'api key not valid', 'invalid_api_key', 'incorrect api key', 'no ') && has('key')
    ? text('Your AI rejected the API key, or no key is saved. Check it in Settings → Your AI.', '你的 AI 拒絕了 API 密鑰，或尚未儲存密鑰。請到「設定 → 你的 AI」檢查。')
    : has('api error 429', 'resource_exhausted', 'rate limit', 'quota')
    ? text('Your AI is rate-limiting or out of quota. Try again shortly.', '你的 AI 暫時限速或額度不足，請稍後再試。')
    : has('error sending request', 'timed out', 'timeout', 'connect', 'dns')
    ? text('Could not reach your AI. Check the network and try again.', '未能連接你的 AI，請檢查網絡後再試。')
    : has('api error 400')
    ? text('Your AI rejected this request. Try again, or choose another model in Settings → Your AI.', '你的 AI 不接受這個請求。請再試一次，或到「設定 → 你的 AI」改用其他模型。')
    : geminiEmpty
    ? text('Gemini returned an empty answer. Try again, or choose another model in Settings → Your AI.', 'Gemini 沒有回覆任何內容。請再試一次，或到「設定 → 你的 AI」改用其他模型。')
    : text('Your AI could not answer just now. Try again shortly.', '你的 AI 暫時未能回覆，請稍後再試。');
  return { message, detail: error };
}
