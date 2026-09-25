import { pathToFileURL } from 'node:url';

export function validateBillingReleaseConfig(env) {
  const names = ['JARVIS_PAYMENT_API', 'JARVIS_COGNITO_DOMAIN', 'JARVIS_COGNITO_CLIENT_ID'];
  return names.filter(name => {
    const raw = env[name];
    if (typeof raw !== 'string' || !raw.trim() || raw !== raw.trim() || /^(none|null|undefined)$/i.test(raw)) return true;
    if (name === 'JARVIS_COGNITO_CLIENT_ID') return !/^[a-zA-Z0-9]{1,128}$/.test(raw);
    // The desktop app appends paths to the original value, not URL's normalized form.
    if (/[?#\s\\]/.test(raw)) return true;
    if (name === 'JARVIS_COGNITO_DOMAIN' && raw.includes('://') && !raw.startsWith('https://')) return true;
    const value = name === 'JARVIS_COGNITO_DOMAIN' && !raw.includes('://') ? `https://${raw}` : raw;
    try {
      const url = new URL(value);
      return url.protocol !== 'https:' || !url.hostname.includes('.') || !!url.username || !!url.password
        || !!url.search || !!url.hash || (name === 'JARVIS_COGNITO_DOMAIN' && url.pathname !== '/');
    } catch {
      return true;
    }
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const invalid = validateBillingReleaseConfig(process.env);
  if (invalid.length) {
    console.error(`付款版建置設定缺失或無效：${invalid.join(', ')}。請先完成設定；下載者不會有開發用 .env。`);
    process.exitCode = 1;
  }
}
