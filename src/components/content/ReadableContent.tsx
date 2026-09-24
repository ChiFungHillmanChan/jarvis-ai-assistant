import { useState } from 'react';
import { ContentBody, type ContentBodyProps } from './content-body';
import { openUrl } from '../../lib/commands';
import { useUiLocale } from '../../i18n';
import './readable-content.css';

export default function ReadableContent({ className = '', ...props }: ContentBodyProps & { className?: string }) {
  const { text } = useUiLocale();
  const [linkError, setLinkError] = useState(false);
  return <div className={`readable-content ${className}`}>
    <ContentBody {...props} tableLabel={text('Table; scroll horizontally for more columns', '表格，可水平捲動查看更多欄位')} onOpenLink={url => {
      setLinkError(false);
      void openUrl(url).catch(() => setLinkError(true));
    }} />
    {linkError && <p className="readable-link-error" role="alert">{text('Could not open the link. Please try again.', '未能開啟連結，請重試。')}</p>}
  </div>;
}
