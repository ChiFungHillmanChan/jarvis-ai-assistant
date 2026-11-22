import { useUiLocale } from '../../../i18n';
import { ROWS_BY_ID, rowText } from '../../rows';

/** A row's DOM id, title and description, from rows.ts in the interface language. */
export function useRow() {
  const { locale } = useUiLocale();
  return (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });
}
