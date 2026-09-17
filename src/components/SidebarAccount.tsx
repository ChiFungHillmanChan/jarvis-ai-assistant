import { useCallback, useEffect, useRef, useState } from 'react';
import { Plug, Settings2 } from 'lucide-react';
import { getSettings } from '../lib/commands';
import { DISPLAY_NAME_KEY, accountBadge, onDisplayNameChanged } from '../lib/account';
import { useUiLocale } from '../i18n';
import type { WorkspaceView } from '../workspace/model';

/**
 * The bottom row of the sidebar: the name from Settings, with 連接服務 and 設定
 * tucked into a menu above it.
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
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(async () => {
    try {
      const settings = await getSettings();
      setName(settings.values[DISPLAY_NAME_KEY] ?? null);
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

  const badge = accountBadge(name, t('navSettings'));
  const go = (view: WorkspaceView) => {
    setOpen(false);
    onNavigate(view);
  };

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
          <button role="menuitem" className="sidebar-item" aria-current={active === 'settings' ? 'page' : undefined} onClick={() => go('settings')}>
            <Settings2 size={16} /><span>{t('navSettings')}</span>
          </button>
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
      </button>
    </div>
  );
}
