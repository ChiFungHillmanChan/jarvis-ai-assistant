import { useEffect, useRef, useState, type ReactNode } from 'react';
import { accountSignIn, accountSignOut, accountStatus, paymentsReopenUrl, paymentsStartCheckout } from '../lib/commands';
import type { AccountStatus, PaymentPage } from '../lib/types';
import { NOT_SIGNED_IN, SIGN_IN_CANCELLED, accountChanged, createAccountOperationGuard, onAccountChanged } from '../lib/account';
import { createAccountMonitor, type PaymentState } from '../lib/account-monitor';
import { gateScreen } from '../lib/paywall';
import { PRO_MONTHLY_PRICE } from '../lib/offer';
import { useUiLocale } from '../i18n';
import WindowControls from './WindowControls';
import '../styles/setup.css';

type Busy = null | 'signin' | 'signout' | 'checkout' | 'code' | 'reopen' | 'refresh';

/**
 * In a billing build nothing past this gate renders until the account has a
 * paid plan (a trial counts). Free and dev builds have no account settings and
 * pass straight through. Design: docs/superpowers/specs/2026-09-27-free-trial-paywall-design.md
 */
export default function PaywallGate({ children }: { children: ReactNode }) {
  const { t, locale } = useUiLocale();
  const [status, setStatus] = useState<AccountStatus | null>(null);
  const [payment, setPayment] = useState<PaymentState>(null);
  const [page, setPage] = useState<PaymentPage | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<string | null>(null);
  const [codeOpen, setCodeOpen] = useState(false);
  const [code, setCode] = useState('');
  const monitor = useRef<ReturnType<typeof createAccountMonitor> | null>(null);
  const operations = useRef(createAccountOperationGuard());

  useEffect(() => {
    const watcher = createAccountMonitor({
      fetchStatus: accountStatus,
      onStatus: next => { setStatus(next); setError(null); },
      onPayment: setPayment,
      onError: e => setError(String(e)),
    });
    monitor.current = watcher;
    void watcher.refresh();
    // A sign-out from Settings, inside the app, brings the gate back.
    const unsubscribe = onAccountChanged(next => { if (next) setStatus(next); else void watcher.refresh(); });
    // Focus checks use the 60-second plan cache; payment waits force fresh reads.
    const resume = () => { if (document.visibilityState !== 'hidden') void watcher.refresh(); };
    window.addEventListener('focus', resume);
    document.addEventListener('visibilitychange', resume);
    const guard = operations.current;
    return () => {
      unsubscribe();
      guard.invalidate();
      window.removeEventListener('focus', resume);
      document.removeEventListener('visibilitychange', resume);
      watcher.dispose();
      monitor.current = null;
    };
  }, []);

  async function run(kind: Exclude<Busy, null>, action: (current: () => boolean) => Promise<void>) {
    operations.current.invalidate();
    setBusy(kind);
    setError(null);
    let current = () => false;
    try {
      // Sign-in/out emits its session boundary synchronously; capture afterwards
      // so this operation survives its own boundary but never a later one.
      const pending = action(() => current());
      current = operations.current.capture();
      await pending;
    } catch (e) {
      if (!current()) return;
      const message = String(e);
      if (message === NOT_SIGNED_IN) {
        monitor.current?.cancelPayment();
        setPage(null);
        await monitor.current?.refresh(true);
      } else if (message === 'invalid_code') setError(t('codeInvalid'));
      else if (message === 'billing_busy') setError(t('billingBusy'));
      else if (message !== SIGN_IN_CANCELLED) setError(message);
    } finally {
      if (current()) setBusy(null);
    }
  }

  const signIn = (provider?: 'google') => run('signin', async current => {
    monitor.current?.cancelPayment();
    const next = await accountSignIn(provider);
    if (!current()) return;
    setStatus(next);
    accountChanged(next);
  });
  const signOut = () => run('signout', async current => {
    monitor.current?.cancelPayment();
    setPage(null);
    await accountSignOut();
    if (current()) await monitor.current?.refresh(true);
  });
  const checkout = (kind: 'checkout' | 'code') => run(kind, async current => {
    monitor.current?.cancelPayment();
    const next = await paymentsStartCheckout('pro', locale, kind === 'code' ? code.trim() : undefined);
    if (!current()) return;
    setPage(next);
    if (next.already_subscribed) await monitor.current?.refresh(true);
    else if (next.browser_opened) monitor.current?.waitForPayment();
    else setError(t('billingBrowserFailed'));
  });
  const reopen = () => run('reopen', async current => {
    if (!page) return;
    const next = await paymentsReopenUrl(page.url, page.session_generation);
    if (!current()) return;
    setPage(next);
    if (next.browser_opened) monitor.current?.waitForPayment();
  });
  const checkAgain = () => run('refresh', async () => { await monitor.current?.refresh(true); });

  const screen = gateScreen(status, payment);
  if (screen === 'open') return <>{children}</>;
  const price = PRO_MONTHLY_PRICE[locale];
  const trialDays = status?.trial_days ?? 0;
  const disabled = busy !== null;

  let body: ReactNode;
  if (screen === 'checking') {
    body = error
      ? <><p className="setup-error" role="alert">{error}</p><button onClick={() => { setError(null); void monitor.current?.refresh(true); }}>{t('retry')}</button></>
      : <p role="status">{t('loading')}</p>;
  } else if (screen === 'sign-in') {
    body = <>
      <h1>{t('gateSignInTitle')}</h1>
      <p>{t('gateSignInBody')}</p>
      <div className="setup-actions">
        <button disabled={disabled} onClick={() => void signIn('google')}>{t('signInGoogle')}</button>
        <button disabled={disabled} onClick={() => void signIn()}>{t('signInEmail')}</button>
      </div>
      {busy === 'signin' && <p role="status">{t('signingIn')}</p>}
    </>;
  } else if (screen === 'offline') {
    body = <>
      <h1>{t('gateOfflineTitle')}</h1>
      <p>{t('gateOfflineBody')}</p>
      <div className="setup-actions"><button disabled={disabled} onClick={() => void checkAgain()}>{busy === 'refresh' ? t('refreshing') : t('retry')}</button></div>
    </>;
  } else if (screen === 'waiting' || screen === 'unconfirmed') {
    body = <>
      <h1>{t('gateWaitingTitle')}</h1>
      <p role="status">{t(screen === 'waiting' ? 'paymentChecking' : 'paymentUnconfirmed')}</p>
      <div className="setup-actions">
        {page && <button disabled={disabled} onClick={() => void reopen()}>{t('reopenCheckout')}</button>}
        {screen === 'unconfirmed' && <button disabled={disabled} onClick={() => void checkAgain()}>{busy === 'refresh' ? t('refreshing') : t('refresh')}</button>}
      </div>
    </>;
  } else {
    body = <>
      <h1>{trialDays > 0 ? t('trialTitle', { days: trialDays }) : t('subscribeTitle')}</h1>
      <p>{trialDays > 0 ? t('trialTerms', { days: trialDays, price }) : t('subscribeTerms', { price })}</p>
      <p>{t('billingByok')}</p>
      <div className="setup-actions">
        <button disabled={disabled} onClick={() => void checkout('checkout')}>{trialDays > 0 ? t('trialStart') : t('subscribe')}</button>
        {page && <button disabled={disabled} onClick={() => void reopen()}>{t('reopenCheckout')}</button>}
      </div>
      {codeOpen
        ? <form className="setup-actions" onSubmit={event => { event.preventDefault(); if (code.trim()) void checkout('code'); }}>
            <label>{t('codeLabel')}<input value={code} maxLength={64} autoComplete="off" spellCheck={false} onChange={event => setCode(event.target.value)} /></label>
            <button type="submit" disabled={disabled || !code.trim()}>{t('codeApply')}</button>
          </form>
        : <button className="text-button" disabled={disabled} onClick={() => setCodeOpen(true)}>{t('codeHave')}</button>}
    </>;
  }

  return <div className="setup-screen">
    <header className="setup-titlebar" data-tauri-drag-region><WindowControls /><span data-tauri-drag-region>JARVIS</span></header>
    <main className="setup-content">
      {body}
      {error && screen !== 'checking' && <p className="setup-error" role="alert">{error}</p>}
      {status?.signed_in && screen !== 'checking' && (
        <p className="setup-footer">{t('signedInAs', { email: status.email ?? '-' })}<button className="text-button" disabled={disabled} onClick={() => void signOut()}>{t('signOut')}</button></p>
      )}
    </main>
  </div>;
}
