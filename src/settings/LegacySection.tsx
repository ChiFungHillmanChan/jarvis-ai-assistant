import { lazy, Suspense } from 'react';
import { useUiLocale } from '../i18n';
import { legacySectionsFor, type SettingsSectionId } from './sections';

// Temporary bridge: a section not rebuilt yet shows the old Settings.tsx body
// for it. Deleted with Settings.tsx (Task 19).
const Settings = lazy(() => import('../pages/Settings'));

export default function LegacySection({ id }: { id: SettingsSectionId }) {
  const { text } = useUiLocale();
  const sections = legacySectionsFor(id);
  if (sections.length === 0) return <p className="settings-dialog-empty">{text('This section moves here soon.', '此部分即將移到這裏。')}</p>;
  return <Suspense fallback={<p className="settings-dialog-empty" role="status">{text('Opening…', '正在開啟…')}</p>}><Settings embedded={{ sections }} /></Suspense>;
}
