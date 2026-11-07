const formats = new Map<string, Intl.DateTimeFormat>();

/** A shared formatter for these options. toLocaleDateString and
 * toLocaleTimeString build a new one on every call, which adds up when a
 * calendar grid or mail list formats a date for every cell or row. */
export function dateFormat(locale: string, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let format = formats.get(key);
  if (!format) formats.set(key, format = new Intl.DateTimeFormat(locale, options));
  return format;
}
