import type { Text } from './mail-presentation';

/**
 * What a failed assistant turn means for the user. The router's own text
 * ("All providers failed: Gemini (...): Gemini API error 400 ...") stays
 * available as technical detail.
 */
export function chatErrorText(error: string, text: Text): { message: string; detail: string | null } {
  if (!error.startsWith('All providers failed')) return { message: error, detail: null };
  const lower = error.toLowerCase();
  const has = (...parts: string[]) => parts.some(part => lower.includes(part));
  const message = has('api error 401', 'api error 403', 'api key not valid', 'invalid_api_key', 'incorrect api key', 'no ') && has('key')
    ? text('Your AI rejected the API key, or no key is saved. Check it in Settings → Your AI.', '你的 AI 拒絕了 API 密鑰，或尚未儲存密鑰。請到「設定 → 你的 AI」檢查。')
    : has('api error 429', 'resource_exhausted', 'rate limit', 'quota')
    ? text('Your AI is rate-limiting or out of quota. Try again shortly.', '你的 AI 暫時限速或額度不足，請稍後再試。')
    : has('error sending request', 'timed out', 'timeout', 'connect', 'dns')
    ? text('Could not reach your AI. Check the network and try again.', '未能連接你的 AI，請檢查網絡後再試。')
    : has('api error 400')
    ? text('Your AI rejected this request. Try again, or choose another model in Settings → Your AI.', '你的 AI 不接受這個請求。請再試一次，或到「設定 → 你的 AI」改用其他模型。')
    : /gemini \([^)]*\) returned no text/.test(lower)
    ? text('Gemini returned an empty answer. Try again, or choose another model in Settings → Your AI.', 'Gemini 沒有回覆任何內容。請再試一次，或到「設定 → 你的 AI」改用其他模型。')
    : text('Your AI could not answer just now. Try again shortly.', '你的 AI 暫時未能回覆，請稍後再試。');
  return { message, detail: error };
}
