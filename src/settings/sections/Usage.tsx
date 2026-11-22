import { useState, type ReactNode } from 'react';
import CostPanel from '../../components/CostPanel';
import { useTauriCommand } from '../../hooks/useTauriCommand';
import { useUiLocale } from '../../i18n';
import { byMonth, emptyPeriodLine, periodLabel, unpricedNote, type CostStats, type PeriodDays, type UsageGroup } from '../../lib/usage';
import Segmented from '../../ui/Segmented';
import Button from '../../ui/Button';
import { ROWS_BY_ID, rowText } from '../rows';
import { sectionLabel } from '../sections';
import { useSettingsDialog } from '../SettingsContext';
import { MAIN_PERIODS, chronological, spentFigure, usageTiles } from '../usage-view';
import { DailyBars, SplitBars } from '../usage/charts';

const count = (n: number) => n.toLocaleString('en-US');

/**
 * Settings > Usage: what this Mac's AI requests cost, as three tiles and two
 * charts, from the same commands as the detail table. "Show details" is that
 * table (CostPanel), sharing this page's period, with 180 days and 2 years.
 */
export default function Usage() {
  const { locale, t, text } = useUiLocale();
  const { openSettings } = useSettingsDialog();
  const [days, setDays] = useState<PeriodDays>(30);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const stats = useTauriCommand<CostStats>("get_cost_stats", { days });
  const features = useTauriCommand<UsageGroup[]>("get_usage_groups", { days, groupBy: "feature" });
  const time = useTauriCommand<UsageGroup[]>("get_usage_groups", { days, groupBy: "time" });
  const row = (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });

  const p = row('usage-period');
  const period = (
    <div id={p.id} className="settings-usage-period">
      <Segmented value={String(days)} label={p.title} onChange={v => setDays(Number(v) as PeriodDays)}
        options={MAIN_PERIODS.map(d => ({ value: String(d), label: periodLabel(d, text) }))} />
    </div>
  );
  const failed = (retry: () => void) => (
    <p className="ui-secret-error" role="alert">
      {text('Could not load usage.', '無法載入用量。')} <Button kind="quiet" onClick={retry}>{t('retry')}</Button>
    </p>
  );
  const loadingLine = <p className="ui-hint" role="status">{text('Loading usage…', '正在載入用量…')}</p>;

  if (stats.error) return <>{period}{failed(stats.refetch)}</>;
  if (!stats.data) return <>{period}{loadingLine}</>;
  if (stats.data.total_requests === 0) {
    const ai = sectionLabel('ai', text);
    return (
      <>
        {period}
        <p className="settings-usage-empty">
          {emptyPeriodLine(days, text)} <Button kind="quiet" onClick={() => openSettings('ai')}>{text(`Go to ${ai}`, `前往「${ai}」`)}</Button>
        </p>
      </>
    );
  }

  const tiles = usageTiles(features.data ?? [], stats.data);
  const spent = spentFigure(tiles.spentUsd, tiles.unpriced, text);
  const requests = (n: number) => (features.data ? count(n) : '…');
  const monthly = byMonth(days);
  const [spentRow, mail, chat] = [row('usage-spent'), row('usage-mail-requests'), row('usage-chat-requests')];
  const daily = row('usage-daily');
  const split = row('usage-breakdown');
  const unpriced = row('usage-unpriced');
  const details = row('usage-details');
  return <>
    {period}
    <div className="settings-usage-tiles">
      <Tile {...spentRow} value={spent.value} detail={spent.usd} />
      <Tile {...mail} value={requests(tiles.mailRequests)} detail={null} hint={text('Sorting new mail', '分類新郵件')} />
      <Tile {...chat} value={requests(tiles.chatRequests)} detail={null} hint={text('Typed or spoken', '文字或語音')} />
    </div>
    {features.error && failed(features.refetch)}

    <Block id={daily.id} title={daily.title}
      hint={monthly ? text('Each bar is one month, in UTC.', '每條代表一個月（UTC）。') : text('Each bar is one day with usage, in UTC.', '每條代表一個有用量的日子（UTC）。')}>
      {time.error ? failed(time.refetch) : time.data ? <DailyBars points={chronological(time.data)} monthly={monthly} caption={daily.title} /> : loadingLine}
    </Block>

    <Block id={split.id} title={split.title} hint={split.description}>
      {features.error ? null : features.data ? <SplitBars groups={features.data} caption={split.title} /> : loadingLine}
    </Block>

    <p id={unpriced.id} className="settings-usage-note">
      {unpricedNote(stats.data, text) ?? text('Every request in this period has a known price.', '此期間所有請求均有已知價格。')}
    </p>

    <details id={details.id} className="settings-usage-details" onToggle={e => setDetailsOpen(e.currentTarget.open)}>
      <summary>{details.title}<span className="ui-hint">{details.description}</span></summary>
      {detailsOpen && <CostPanel days={days} onDaysChange={setDays} />}
    </details>
  </>;
}

/** A figure with its label; `detail` sits small beside the value (the US$), `hint` under it. */
function Tile({ id, title, description, value, detail, hint }: { id: string; title: string; description?: string; value: string; detail: string | null; hint?: string }) {
  return (
    <div id={id} className="settings-usage-tile">
      <div className="settings-usage-tile-label">{title}</div>
      <div className="settings-usage-tile-value">{value}{detail && <span className="settings-usage-tile-detail"> {detail}</span>}</div>
      {(hint ?? description) && <div className="ui-hint">{hint ?? description}</div>}
    </div>
  );
}

function Block({ id, title, hint, children }: { id: string; title: string; hint?: string; children: ReactNode }) {
  return (
    <section id={id} className="settings-usage-block" aria-labelledby={`${id}-title`}>
      <h3 className="ui-setting-group-title" id={`${id}-title`}>{title}</h3>
      {hint && <p className="ui-hint">{hint}</p>}
      {children}
    </section>
  );
}
