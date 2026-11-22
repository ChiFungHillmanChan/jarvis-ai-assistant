export interface SelectOption<V extends string = string> {
  value: V;
  label: string;
  hint?: string;
  group?: string;
  disabled?: boolean;
}

/** The label, or `label · hint` when the option carries a hint. */
export function formatOption(o: SelectOption): string {
  return o.hint ? `${o.label} · ${o.hint}` : o.label;
}

/** Consecutive options that share a group become one run; the order is kept. */
export function groupOptions<V extends string>(opts: SelectOption<V>[]): { group: string | null; items: SelectOption<V>[] }[] {
  const runs: { group: string | null; items: SelectOption<V>[] }[] = [];
  for (const o of opts) {
    const group = o.group ?? null;
    const last = runs[runs.length - 1];
    if (last && last.group === group) last.items.push(o);
    else runs.push({ group, items: [o] });
  }
  return runs;
}
