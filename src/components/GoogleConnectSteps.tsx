import type { CSSProperties } from 'react';
import { useUiLocale } from '../i18n';

/**
 * What the user meets on Google's pages, said before they get there. Calendar
 * is an unticked checkbox on Google's consent screen, and the unverified-app
 * notice hides the way on under Advanced; both stopped people in testing.
 */
export default function GoogleConnectSteps({ className, style }: { className?: string; style?: CSSProperties }) {
  const { text } = useUiLocale();
  return <ol className={className} style={{ paddingLeft: 18, lineHeight: 1.6, ...style }}>
    <li>{text('Google opens in your browser. Choose your Google account.', 'Google 會在瀏覽器開啟。請選擇你的 Google 帳戶。')}</li>
    <li>{text('While Google reviews JARVIS, it shows "Google hasn\'t verified this app". Click Advanced, then Go to JARVIS.', 'Google 審核 JARVIS 期間會顯示「Google 尚未驗證這個應用程式」。請按「進階」（Advanced），再按「前往 JARVIS」（Go to JARVIS）。')}</li>
    <li>{text('Tick "View and edit events on all your calendars", then Continue. Without it JARVIS cannot see your calendar.', '剔選日曆權限（View and edit events on all your calendars），再按「繼續」。未剔選的話，JARVIS 無法讀取你的日曆。')}</li>
  </ol>;
}
