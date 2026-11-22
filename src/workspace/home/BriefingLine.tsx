import { useId, useState } from 'react';
import MessageRenderer from '../../components/chat/MessageRenderer';
import { useUiLocale } from '../../i18n';
import { localDate } from '../../lib/follow-ups';
import type { DailyBrief } from '../../lib/types';
import { briefingSummary } from './home-logic';

function shownAt(value: string, locale: string): string {
  const d = new Date(value.includes('T') || /Z$/.test(value) ? value : value.replace(' ', 'T') + 'Z');
  return isNaN(d.getTime()) ? value : d.toLocaleString(locale, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}

/**
 * One line from today's briefing, with the full brief shown in place on
 * Expand. Not shown when there is no brief for today. The mail figure in
 * the line is unread mail (briefingSummary), never "needs a reply".
 */
export default function BriefingLine({ brief }: { brief: DailyBrief | null }) {
  const { text, t, locale } = useUiLocale();
  const [open, setOpen] = useState(false);
  const id = useId();
  const summary = briefingSummary(brief, text, localDate(new Date()));
  if (!brief || !summary) return null;
  return <section className="home-brief" aria-label={t('briefTitle')}>
    <p className="home-brief-line">
      <span>{summary}</span>
      <button type="button" className="text-button" aria-expanded={open} aria-controls={id} onClick={() => setOpen(v => !v)}>
        {open ? text('Collapse', '收起') : text('Expand', '展開')}
      </button>
    </p>
    {open && <div className="home-brief-full" id={id}>
      <small>{shownAt(brief.shown_at, locale)}</small>
      {brief.summary && <div><MessageRenderer content={brief.summary} /></div>}
      <pre>{brief.local}</pre>
      {brief.summary_pending && <small role="status">{t('briefSummaryPending')}</small>}
      {brief.summary_error && <small>{t('briefSummaryFailed', { reason: brief.summary_error })}</small>}
      {(brief.trigger === 'catch_up' || brief.trigger === 'retry_interrupted') && <small>{t('briefShownLate')}</small>}
    </div>}
  </section>;
}
