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
import { MAIN_PERIODS, dailyTitle, fillDays, spentFigure, usageTiles } from '../usage-view';
import { DailyBars, SplitBars } from '../usage/charts';

const count = (n: number) => n.toLocaleString('en-US');

/**
 * Settings > Usage: what this Mac's AI requests cost, as three tiles and two
 * charts, from the same commands as the detail table. "Show details" is that
 * table (CostPanel) as it is, with its own period: 180 days and 2 years live there.
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
  const monthly = byMonth(days);

  // Every row, in display order.
  const p = row('usage-period');
  const [spentRow, mail, chat] = [row('usage-spent'), row('usage-mail-requests'), row('usage-chat-requests')];
  const daily = row('usage-daily');
  const split = row('usage-breakdown');
  const unpriced = row('usage-unpriced');
  const details = row('usage-details');

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
  const ai = sectionLabel('ai', text);
  const empty = (
    <p className="settings-usage-empty">
      {emptyPeriodLine(days, text)} <Button kind="quiet" onClick={() => openSettings('ai')}>{text(`Go to ${ai}`, `前往「${ai}」`)}</Button>
    </p>
  );

  function figures(data: CostStats) {
    const tiles = usageTiles(features.data ?? [], data);
    const spent = spentFigure(tiles.spentUsd, tiles.unpriced, text);
    const requests = (n: number) => (features.error ? '–' : features.data ? count(n) : '…');
    const chartTitle = dailyTitle(monthly, text);
    return <>
      <div className="settings-usage-tiles">
        <Tile {...spentRow} value={spent.value} detail={spent.usd} />
        <Tile {...mail} value={requests(tiles.mailRequests)} detail={null} hint={text('Sorting new mail', '分類新郵件')} />
        <Tile {...chat} value={requests(tiles.chatRequests)} detail={null} hint={text('Typed or spoken', '文字或語音')} />
      </div>
      {features.error && failed(features.refetch)}

      <Block id={daily.id} title={chartTitle} hint={text('Counted in UTC.', '按 UTC 計算。')}>
        {time.error ? failed(time.refetch) : time.data ? <DailyBars points={fillDays(time.data)} monthly={monthly} caption={chartTitle} /> : loadingLine}
      </Block>

      <Block id={split.id} title={split.title} hint={split.description}>
        {features.error ? null : features.data ? <SplitBars groups={features.data} caption={split.title} /> : loadingLine}
      </Block>

      <p id={unpriced.id} className="settings-usage-note">
        {unpricedNote(data, text) ?? text('Every request in this period has a known price.', '此期間所有請求均有已知價格。')}
      </p>
    </>;
  }

  const body = stats.error ? failed(stats.refetch)
    : !stats.data ? loadingLine
    : stats.data.total_requests === 0 ? empty
    : figures(stats.data);

  // Show details is outside the figures, so changing the period never closes
  // it. The panel keeps its own period, which also offers 180 days and 2 years.
  return <>
    {period}
    {body}
    <p className="ui-hint settings-usage-about">{text(
      "The AI requests JARVIS sent from this Mac, recorded only on this Mac. Costs are worked out from list prices where JARVIS knows them, so your provider's bill is the final figure. Token counts show input without the cache, output, and cache reads and writes when there are any. For AWS Bedrock these are the figures AWS returned for each request (input is AWS's inputTokens), and your AWS bill is the final figure.",
      'JARVIS 在此 Mac 發出的 AI 請求，只記錄在此 Mac。費用按 JARVIS 已知的公開價目計算，實際收費以供應商帳單為準。token 數目分開顯示不含快取的輸入、輸出，以及快取讀取及寫入（如有）。AWS Bedrock 的數字是 AWS 就每次請求回報的數字（輸入即 AWS 的 inputTokens），實際收費以你的 AWS 帳單為準。',
    )}</p>
    <details id={details.id} className="settings-usage-details" onToggle={e => setDetailsOpen(e.currentTarget.open)}>
      <summary>{details.title}<span className="ui-hint">{details.description}</span></summary>
      {detailsOpen && <CostPanel />}
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
