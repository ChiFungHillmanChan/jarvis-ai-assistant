import Button from '../../ui/Button';
import { useUiLocale } from '../../i18n';
import { useSettingsDialog } from '../../settings/SettingsContext';
import type { HomeState } from './home-logic';

/** The one card both views show until Google and an AI are set up. */
export default function HomeSetupCard({ state }: { state: Exclude<HomeState, 'ready'> }) {
  const { text } = useUiLocale();
  const { openSettings, openSetupGuide } = useSettingsDialog();
  return <section className="home-setup" aria-label={text('Set up JARVIS', '設定 JARVIS')}>
    {state === 'needs-google' ? <>
      <p>{text('Connect Gmail and JARVIS will find the emails that need a reply', '連接 Gmail，JARVIS 便會找出需要回覆的郵件')}</p>
      <Button kind="primary" onClick={() => openSettings('connections')}>{text('Connect Gmail', '連接 Gmail')}</Button>
    </> : <>
      <p>{text('Choose an AI so JARVIS can read and sort your mail', '選擇一個 AI，JARVIS 便可閱讀及整理你的郵件')}</p>
      <Button kind="primary" onClick={() => openSetupGuide()}>{text('Set up', '設定')}</Button>
    </>}
  </section>;
}
