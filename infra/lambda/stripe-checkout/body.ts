/**
 * The only thing checkout reads from the body. The user id and email come from
 * the verified token and from Cognito; any identity fields a caller sends are
 * ignored, which is what makes the endpoint safe to expose.
 */
export function readTier(body: string | undefined, isBase64: boolean): 'pro' | 'team' | null {
  if (!body) return null;
  try {
    const parsed = JSON.parse(isBase64 ? Buffer.from(body, 'base64').toString('utf8') : body);
    return parsed?.tier === 'pro' || parsed?.tier === 'team' ? parsed.tier : null;
  } catch {
    return null;
  }
}

/** Restrict return paths to the website's supported languages. */
export function readLocale(body: string | undefined, isBase64: boolean): 'en' | 'zh-HK' {
  try {
    const parsed = JSON.parse(isBase64 ? Buffer.from(body || '', 'base64').toString('utf8') : body || '{}');
    return parsed?.locale === 'zh-HK' ? 'zh-HK' : 'en';
  } catch { return 'en'; }
}

/** Longest promotion code the app may send; longer input is refused without a Stripe call. */
export const MAX_CODE_LENGTH = 64;

/**
 * A promotion code typed in the app. Missing or blank means a normal checkout
 * (null); anything that cannot be a code is false.
 */
export function readCode(body: string | undefined, isBase64: boolean): string | null | false {
  try {
    const parsed = JSON.parse(isBase64 ? Buffer.from(body || '', 'base64').toString('utf8') : body || '{}');
    if (parsed?.code === undefined || parsed?.code === null) return null;
    if (typeof parsed.code !== 'string') return false;
    const code = parsed.code.trim();
    if (!code) return null;
    return code.length <= MAX_CODE_LENGTH ? code : false;
  } catch { return null; }
}
