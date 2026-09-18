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
