import { useUiLocale } from '../../i18n';
import { featureLabel, timeLabel, type UsageGroup } from '../../lib/usage';
import { barHeights, dailySummary, formatMoney, splitShares, splitSummary } from '../usage-view';

// Hand-built charts for Settings > Usage: plain elements, no chart library.
// Each chart is one image to a screen reader, named by a summary sentence,
// and a visually hidden table beside it holds the same numbers. The table is
// wrapped in the hidden div because a table box ignores the 1 px size and overflow.

const BAR_MAX_PX = 120;

/** The known cost and, when some requests had no price, how many. */
function costCell(group: UsageGroup, text: (en: string, zh: string) => string): string {
  const money = formatMoney(group.usd_cost);
  const unpriced = group.price_unknown_requests;
  return unpriced > 0 ? text(`${money}, ${unpriced} with no price`, `${money}，${unpriced} 次沒有價格`) : money;
}

/** Cost per day, or per month past 90 days. `points` are oldest first. */
export function DailyBars({ points, monthly, caption }: { points: UsageGroup[]; monthly: boolean; caption: string }) {
  const { text } = useUiLocale();
  const values = points.map(p => p.usd_cost);
  const heights = barHeights(values, BAR_MAX_PX);
  const summary = dailySummary(points, text, monthly);
  const when = (p: UsageGroup) => timeLabel(p.key, text, p.from_day);
  if (heights.every(h => h === 0)) return <p className="ui-hint">{summary}</p>;
  return (
    <div className="settings-usage-chart">
      <div role="img" aria-label={summary}>
        <div className="settings-usage-scale" aria-hidden="true">{formatMoney(Math.max(...values))}</div>
        <div className="settings-usage-bars" style={{ height: BAR_MAX_PX }}>
          {points.map((p, i) => (
            <span key={p.key} className="settings-usage-bar" style={{ height: heights[i] }} title={`${when(p)}: ${formatMoney(p.usd_cost)}`} />
          ))}
        </div>
        <div className="settings-usage-dates" aria-hidden="true">
          <span>{when(points[0])}</span>
          {points.length > 1 && <span>{when(points[points.length - 1])}</span>}
        </div>
      </div>
      <div className="visually-hidden"><table>
        <caption>{caption}</caption>
        <thead>
          <tr>
            <th scope="col">{monthly ? text('Month', '月份') : text('Day', '日期')}</th>
            <th scope="col">{text('Cost', '費用')}</th>
            <th scope="col">{text('Requests', '請求次數')}</th>
          </tr>
        </thead>
        <tbody>
          {points.map(p => (
            <tr key={p.key}><th scope="row">{when(p)}</th><td>{costCell(p, text)}</td><td>{p.requests}</td></tr>
          ))}
        </tbody>
      </table></div>
    </div>
  );
}

/** Where the known cost went, by feature, largest first. */
export function SplitBars({ groups, caption }: { groups: UsageGroup[]; caption: string }) {
  const { text } = useUiLocale();
  const shares = splitShares(groups).filter(s => s.usd > 0);
  const summary = splitSummary(shares, text);
  if (shares.length === 0) return <p className="ui-hint">{summary}</p>;
  const percent = (share: number) => `${Math.round(share * 100)}%`;
  return (
    <div className="settings-usage-chart">
      <div role="img" aria-label={summary} className="settings-usage-split">
        {shares.map(s => (
          <div key={s.feature} className="settings-usage-split-row">
            <span className="settings-usage-split-label">{featureLabel(s.feature, text)}</span>
            <span className="settings-usage-track"><span className="settings-usage-fill" style={{ width: `${s.share * 100}%` }} /></span>
            <span className="settings-usage-split-amount">{formatMoney(s.usd)}</span>
          </div>
        ))}
      </div>
      <div className="visually-hidden"><table>
        <caption>{caption}</caption>
        <thead>
          <tr>
            <th scope="col">{text('Feature', '功能')}</th>
            <th scope="col">{text('Cost', '費用')}</th>
            <th scope="col">{text('Share', '佔比')}</th>
          </tr>
        </thead>
        <tbody>
          {shares.map(s => (
            <tr key={s.feature}><th scope="row">{featureLabel(s.feature, text)}</th><td>{formatMoney(s.usd)}</td><td>{percent(s.share)}</td></tr>
          ))}
        </tbody>
      </table></div>
    </div>
  );
}
