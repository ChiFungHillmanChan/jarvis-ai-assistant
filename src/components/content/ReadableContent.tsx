import { useMemo, useState } from 'react';
import { ContentBody, linkHost, type ContentBodyProps } from './content-body';
import { openUrl } from '../../lib/commands';
import { useUiLocale } from '../../i18n';
import './readable-content.css';

export default function ReadableContent({ className = '', ...props }: ContentBodyProps & { className?: string }) {
  const { text } = useUiLocale();
  const [linkError, setLinkError] = useState(false);
  // Links here are written by the AI or by whoever wrote the page it read. The
  // text can hide the address, and a query string can carry the user's mail
  // out, so the full address is shown and the user decides.
  const [pendingLink, setPendingLink] = useState<string | null>(null);
  const openPending = () => {
    if (!pendingLink) return;
    setPendingLink(null);
    void openUrl(pendingLink).catch(() => setLinkError(true));
  };
  // Stable while the locale is, so ContentBody keeps its rendered mail.
  const quoteLabels = useMemo(() => ({
    show: (count: number) => count ? text(`Show ${count} earlier message${count === 1 ? '' : 's'}`, `顯示較早的 ${count} 封郵件`) : text('Show quoted text', '顯示引用內容'),
    hide: text('Hide quoted text', '隱藏引用內容'),
  }), [text]);
  return <div className={`readable-content ${className}`}>
    <ContentBody {...props} tableLabel={text('Table; scroll horizontally for more columns', '表格，可水平捲動查看更多欄位')} quoteLabels={quoteLabels} onOpenLink={url => {
      setLinkError(false);
      setPendingLink(url);
    }} />
    {pendingLink && <div className="readable-link-confirm" role="alertdialog" aria-labelledby="readable-link-confirm-title">
      <p id="readable-link-confirm-title">{text('Open this link?', '開啟這個連結？')} <strong>{linkHost(pendingLink)}</strong></p>
      <code>{pendingLink}</code>
      <div className="readable-link-confirm-actions">
        <button type="button" className="update-toast-action" onClick={openPending}>{text('Open', '開啟')}</button>
        <button type="button" className="tool-approval-decline" onClick={() => setPendingLink(null)}>{text('Cancel', '取消')}</button>
      </div>
    </div>}
    {linkError && <p className="readable-link-error" role="alert">{text('Could not open the link. Please try again.', '未能開啟連結，請重試。')}</p>}
  </div>;
}
