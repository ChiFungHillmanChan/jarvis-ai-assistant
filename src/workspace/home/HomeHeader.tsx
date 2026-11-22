import { useState } from 'react';
import Segmented from '../../ui/Segmented';
import { updateSetting } from '../../lib/commands';
import { useUiLocale } from '../../i18n';
import type { HomeView } from './home-logic';

function greeting(hour: number, name: string | null, text: (en: string, zh: string) => string): string {
  const [en, zh] = hour < 12 ? ['Good morning', '早安'] : hour < 18 ? ['Good afternoon', '午安'] : ['Good evening', '晚上好'];
  return name ? text(`${en}, ${name}`, `${name}，${zh}`) : text(en, zh);
}

/**
 * The greeting, today's date and the Overview | Focus switch. The switch
 * moves at once and is saved in pref home_view; a failed save puts the
 * previous view back and says so.
 */
export default function HomeHeader({ name, view, onView, needCount }: { name: string | null; view: HomeView; onView(v: HomeView): void; needCount: number | null }) {
  const { text, locale } = useUiLocale();
  const [error, setError] = useState('');
  const now = new Date();

  async function choose(next: HomeView) {
    if (next === view) return;
    const previous = view;
    onView(next);
    setError('');
    try { await updateSetting('home_view', next); }
    catch {
      onView(previous);
      setError(text('Could not save your choice of view. Please try again.', '未能儲存所選的檢視，請再試一次。'));
    }
  }

  const date = now.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long' });
  const need = !needCount ? null
    : text(`${needCount} ${needCount === 1 ? 'thing needs' : 'things need'} you today`, `今天有 ${needCount} 項事情需要你處理`);

  return <header className="home-header">
    <div className="home-header-text">
      <h1>{greeting(now.getHours(), name, text)}</h1>
      <p>{date}{need && <> · <strong>{need}</strong></>}</p>
    </div>
    <Segmented label={text('Home view', '主頁檢視')} value={view} onChange={v => void choose(v)}
      options={[{ value: 'overview', label: text('Overview', '總覽') }, { value: 'focus', label: text('Focus', '重點') }]} />
    {error && <p className="home-error" role="alert">{error}</p>}
  </header>;
}
