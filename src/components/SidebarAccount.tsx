import { useCallback, useEffect, useRef, useState } from 'react';
import { LogIn, LogOut, Plug, Settings2 } from 'lucide-react';
import { accountSignIn, accountSignOut, accountStatus, getSettings } from '../lib/commands';
import type { AccountStatus } from '../lib/types';
import { SIGN_IN_CANCELLED, accountChanged, accountLabel, onAccountChanged, onDisplayNameChanged, isAccountSessionChanging, onAccountSessionChange, tierKey } from '../lib/account';
import { useUiLocale } from '../i18n';
import { nameToShow } from '../lib/user-name';
import { createAccountMonitor } from '../lib/account-monitor';
import type { WorkspaceView } from '../workspace/model';
import { useSettingsDialog } from '../settings/SettingsContext';
import { confirmSignOut } from './ConfirmDialog';

/**
 * The bottom row of the sidebar: the user's name (nameToShow: what JARVIS calls you, then a typed display name, then the Google name) or the signed-in account's email, with 連接服務, 設定 and sign-in tucked into a menu above it.
 *
 * Hover opens the menu, but hover is never the only way in -- click toggles it
 * and the trigger is a real button, so the two destinations stay reachable by
 * keyboard. Closing on pointer leave is delayed slightly so the diagonal
 * travel from the trigger to a menu item does not dismiss it mid-move.
 */
const CLOSE_DELAY_MS = 120;

export default function SidebarAccount({ active, onNavigate }: { active: WorkspaceView | undefined; onNavigate: (view: WorkspaceView) => void }) {
  const { t } = useUiLocale();
  const [name, setName] = useState<string | null>(null);
  const [account, setAccount] = useState<AccountStatus | null>(null);
  const [accountBusy, setAccountBusy] = useState(false);
  const [sessionChanging, setSessionChanging] = useState(isAccountSessionChanging);
  useEffect(() => {
    const watcher = createAccountMonitor({
      fetchStatus: accountStatus,
      onStatus: setAccount,
      onPayment: () => {},
      onError: () => setAccount(null),
    });
    void watcher.refresh();
    const unsubscribe = onAccountChanged(next => {
      if (next) { watcher.cancelPayment(); setAccount(next); }
      else void watcher.refresh();
    });
    const stopSession = onAccountSessionChange(changing => {
      setSessionChanging(changing);
      watcher.cancelPayment();
      if (!changing) void watcher.refresh(true);
    });
    // This row stays mounted outside Settings. A return from Stripe must
    // bypass the plan cache even after the Account panel was closed.
    // The monitor coalesces simultaneous focus/visibility requests.
    const resume = () => { if (document.visibilityState !== 'hidden') void watcher.refresh(true); };
    window.addEventListener('focus', resume);
    document.addEventListener('visibilitychange', resume);
    return () => {
      unsubscribe();
      stopSession();
      watcher.dispose();
      window.removeEventListener('focus', resume);
      document.removeEventListener('visibilitychange', resume);
    };
  }, []);
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(async () => {
    try {
      const settings = await getSettings();
      setName(nameToShow(settings.values));
    } catch {
      // An unreadable settings row is not worth an error state here; the row
      // simply shows its 設定 fallback and the menu still works.
      setName(null);
    }
  }, []);

  useEffect(() => {
    void load();
    return onDisplayNameChanged(() => void load());
  }, [load]);

  const cancelClose = useCallback(() => {
    if (closeTimer.current) {
      clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  }, []);

  const scheduleClose = useCallback(() => {
    cancelClose();
    closeTimer.current = setTimeout(() => setOpen(false), CLOSE_DELAY_MS);
  }, [cancelClose]);

  useEffect(() => cancelClose, [cancelClose]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!wrapRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  const signedIn = Boolean(account?.signed_in);
  const badge = accountLabel(name, signedIn ? account?.email : null, t('navSettings'));
  const signInOrOut = async () => {
    if (accountBusy || sessionChanging) return;
    setOpen(false);
    // Spec 6.3: sign out asks first, with the same question as Settings.
    if (signedIn && !(await confirmSignOut(t))) return;
    setAccountBusy(true);
    try {
      if (signedIn) await accountSignOut();
      else await accountSignIn();
    } catch (e) {
      if (String(e) !== SIGN_IN_CANCELLED) console.warn('account action failed', e);
    } finally {
      setAccountBusy(false);
      accountChanged();
    }
  };
  const go = (view: WorkspaceView) => {
    setOpen(false);
    onNavigate(view);
  };
  const { openSettings } = useSettingsDialog();

  return (
    <div
      className="sidebar-account"
      data-open={open}
      ref={wrapRef}
      onMouseEnter={() => { cancelClose(); setOpen(true); }}
      onMouseLeave={scheduleClose}
    >
      {open && (
        <div className="sidebar-account-menu" role="menu" aria-label={t('accountMenu')}>
          <button role="menuitem" className="sidebar-item" aria-current={active === 'connections' ? 'page' : undefined} onClick={() => go('connections')}>
            <Plug size={16} /><span>{t('navConnections')}</span>
          </button>
          <button role="menuitem" className="sidebar-item" onClick={() => { setOpen(false); openSettings(); }}>
            <Settings2 size={16} /><span>{t('navSettings')}</span>
          </button>
          {account?.available && (
            <button role="menuitem" className="sidebar-item" disabled={accountBusy || sessionChanging} onClick={() => void signInOrOut()}>
              {signedIn ? <LogOut size={16} /> : <LogIn size={16} />}<span>{signedIn ? t('signOut') : t('signIn')}</span>
            </button>
          )}
        </div>
      )}
      <button
        type="button"
        className="sidebar-account-trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(v => !v)}
        onFocus={cancelClose}
      >
        <span className="sidebar-account-avatar" aria-hidden="true">
          {badge.initial ?? <Settings2 size={15} />}
        </span>
        <span className="sidebar-account-name">{badge.label}</span>
        {signedIn && <span className="sidebar-account-tier">{t(tierKey(account?.tier))}</span>}
      </button>
    </div>
  );
}
