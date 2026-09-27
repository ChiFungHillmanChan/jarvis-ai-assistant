import type { UiLocale } from '../i18n/catalog';

/**
 * What Pro costs, as the paywall shows it. Stripe's checkout page states the
 * binding terms; change this together with the live Stripe price.
 */
export const PRO_MONTHLY_PRICE: Record<UiLocale, string> = { en: 'HK$148/month', 'zh-HK': '每月 HK$148' };
