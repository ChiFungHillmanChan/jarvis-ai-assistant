import { useCallback, useEffect, useRef, useState, type ComponentType } from 'react';
import { X } from 'lucide-react';
import ConfirmDialog from '../components/ConfirmDialog';
import ErrorBoundary from '../components/ErrorBoundary';
import Select from '../ui/Select';
import type { SelectOption } from '../ui/options';
import { useUiLocale } from '../i18n';
import { useSettingsDialog } from './SettingsContext';
import { ADVANCED_SECTIONS, NAV_GROUPS, advancedGroupLabel, sectionLabel, type SettingsSectionId } from './sections';
import { ROWS, rowText, type RowMeta } from './rows';
import { searchRows } from './search';
import LegacySection from './LegacySection';
import General from './sections/General';
import Connections from './sections/Connections';
import Privacy from './sections/Privacy';
import SetupGuide from './sections/SetupGuide';
import About from './sections/About';
import { isBackdropClick } from './settings-dialog-state';
import './settings.css';

// The one place a section maps to its component. A section without an entry
// shows the old page's body through LegacySection until it is rebuilt.
const SECTION_VIEWS: Partial<Record<SettingsSectionId, ComponentType>> = {
  general: General,
  connections: Connections,
  privacy: Privacy,
  setup: SetupGuide,
  about: About,
};

export default function SettingsDialog() {
  const { locale, text } = useUiLocale();
  const { open, section, openSettings, closeSettings } = useSettingsDialog();
  const dialog = useRef<HTMLDialogElement>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);
  const confirmEscape = useRef(false);
  const scrollTarget = useRef<string | null>(null);
  const pointerDown = useRef<EventTarget | null>(null);
  const [query, setQuery] = useState('');

  // Runs on every close, however it happened. The ref is cleared before
  // focusing, so the state close and the native close event focus once.
  const finishClose = useCallback(() => {
    setQuery('');
    const target = restoreFocus.current;
    restoreFocus.current = null;
    target?.focus();
  }, []);

  useEffect(() => {
    const el = dialog.current;
    if (!el) return;
    if (open) {
      if (!el.open) {
        restoreFocus.current = document.activeElement as HTMLElement | null;
        el.showModal();
      }
    } else {
      if (el.open) el.close();
      finishClose();
    }
  }, [open, finishClose]);

  // A close that did not go through closeSettings (for example the browser
  // closing the dialog itself on a repeated Esc) still updates the state, so
  // Settings can open again.
  useEffect(() => {
    const el = dialog.current;
    if (!el) return;
    function onClose() { closeSettings(); finishClose(); }
    el.addEventListener('close', onClose);
    return () => el.removeEventListener('close', onClose);
  }, [closeSettings, finishClose]);

  // Esc arrives as the native cancel event. The dialog closes through state,
  // so the native close is always prevented. An Esc that answers a
  // confirmation inside the dialog closes only that confirmation.
  useEffect(() => {
    const el = dialog.current;
    if (!el) return;
    function cancel(e: Event) {
      e.preventDefault();
      if (confirmEscape.current) { confirmEscape.current = false; return; }
      closeSettings();
    }
    el.addEventListener('cancel', cancel);
    return () => el.removeEventListener('cancel', cancel);
  }, [closeSettings]);

  // After a search result opens its section, bring that row into view. Rows in
  // rebuilt sections carry their row id as the DOM id; bridge sections do not.
  useEffect(() => {
    const id = scrollTarget.current;
    if (!id || query) return;
    scrollTarget.current = null;
    requestAnimationFrame(() => dialog.current?.querySelector(`#${CSS.escape(id)}`)?.scrollIntoView({ block: 'center' }));
  }, [section, query]);

  function go(id: SettingsSectionId) {
    setQuery('');
    openSettings(id);
  }

  function choose(row: RowMeta) {
    scrollTarget.current = row.id;
    go(row.section);
  }

  const inAdvanced = ADVANCED_SECTIONS.includes(section);
  const navButton = (id: SettingsSectionId) => (
    <button type="button" className="settings-dialog-nav-item" aria-current={id === section && !query ? 'page' : undefined} onClick={() => go(id)}>
      {sectionLabel(id, text)}
    </button>
  );
  const navOptions: SelectOption<SettingsSectionId>[] = [
    ...NAV_GROUPS.flat().map(id => ({ value: id, label: sectionLabel(id, text) })),
    ...ADVANCED_SECTIONS.map(id => ({ value: id, label: sectionLabel(id, text), group: advancedGroupLabel(text) })),
  ];
  const searching = query.trim() !== '';
  const results = searching ? searchRows(query, ROWS, locale) : [];
  const View = SECTION_VIEWS[section];

  return (
    <dialog
      ref={dialog}
      className="settings-dialog"
      aria-labelledby="settings-dialog-title"
      onPointerDown={e => { pointerDown.current = e.target; }}
      onClick={e => {
        if (dialog.current && isBackdropClick(pointerDown.current, e.target, dialog.current, dialog.current.getBoundingClientRect(), e.clientX, e.clientY)) closeSettings();
        pointerDown.current = null;
      }}
      onKeyDown={e => { if (e.key === 'Escape') confirmEscape.current = !!dialog.current?.querySelector('.confirm-backdrop'); }}
    >
      {open && <>
        <div className="settings-dialog-frame">
          <div className="settings-dialog-inner">
            <nav className="settings-dialog-nav" aria-label={text('Settings sections', '設定分類')}>
              <input
                type="search"
                className="ui-input settings-dialog-search"
                aria-label={text('Search settings', '搜尋設定')}
                placeholder={text('Search settings', '搜尋設定')}
                value={query}
                onChange={e => setQuery(e.target.value)}
              />
              <div className="settings-dialog-nav-list">
                {NAV_GROUPS.map(group => (
                  <ul key={group[0]} className="settings-dialog-nav-group">
                    {group.map(id => <li key={id}>{navButton(id)}</li>)}
                  </ul>
                ))}
                <div className="settings-dialog-nav-advanced">
                  <button type="button" className="settings-dialog-nav-item" aria-expanded={inAdvanced} onClick={() => go('advanced-models')}>
                    {advancedGroupLabel(text)}
                  </button>
                  {inAdvanced && (
                    <ul className="settings-dialog-nav-sub">
                      {ADVANCED_SECTIONS.map(id => <li key={id}>{navButton(id)}</li>)}
                    </ul>
                  )}
                </div>
              </div>
              <div className="settings-dialog-nav-select">
                <Select value={section} options={navOptions} onChange={go} label={text('Settings section', '設定分類')} width="100%" />
              </div>
            </nav>
            <div className="settings-dialog-pane">
              <header className="settings-dialog-header">
                <h2 id="settings-dialog-title">{searching ? text('Search results', '搜尋結果') : sectionLabel(section, text)}</h2>
                <button type="button" className="icon-button" aria-label={text('Close settings', '關閉設定')} onClick={closeSettings}><X size={18} /></button>
              </header>
              <div className="settings-dialog-body">
                {searching ? (
                  results.length === 0
                    ? <p className="settings-dialog-empty">{text('No settings match your search.', '沒有符合搜尋的設定。')}</p>
                    : results.map(group => (
                      <section key={group.section} className="settings-dialog-results">
                        <h3>{sectionLabel(group.section, text)}</h3>
                        <ul>
                          {group.rows.map(row => {
                            const label = rowText(row, locale);
                            return (
                              <li key={row.id}>
                                <button type="button" onClick={() => choose(row)}>
                                  <span>{label.title}</span>
                                  {label.description && <small>{label.description}</small>}
                                </button>
                              </li>
                            );
                          })}
                        </ul>
                      </section>
                    ))
                ) : (
                  <ErrorBoundary key={section}>{View ? <View /> : <LegacySection id={section} />}</ErrorBoundary>
                )}
              </div>
            </div>
          </div>
        </div>
        {/* The workspace behind is inert while the dialog is open, so the
            confirmations a section asks for show here instead (App.tsx
            renders its own only while the dialog is closed). */}
        <ConfirmDialog />
      </>}
    </dialog>
  );
}
