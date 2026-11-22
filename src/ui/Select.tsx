import { useId } from 'react';
import { formatOption, groupOptions, type SelectOption } from './options';
import './ui.css';

interface SelectProps<V extends string> {
  value: V;
  options: SelectOption<V>[];
  onChange(v: V): void;
  label: string;
  showLabel?: boolean;
  disabled?: boolean;
  disabledReason?: string;
  width?: number | string;
  id?: string;
  describedBy?: string;
  required?: boolean;
}

export default function Select<V extends string>({ value, options, onChange, label, showLabel, disabled, disabledReason, width, id, describedBy, required }: SelectProps<V>) {
  const generated = useId();
  const selectId = id ?? generated;
  const reasonId = `${selectId}-reason`;
  const reason = disabled ? disabledReason : undefined;
  const described = [describedBy, reason ? reasonId : undefined].filter(Boolean).join(' ') || undefined;
  return (
    <div className="ui-field">
      {showLabel && <label className="ui-label" htmlFor={selectId}>{label}</label>}
      <div className="ui-select-wrap" style={{ width }}>
        <select
          id={selectId}
          className="ui-select"
          value={value}
          disabled={disabled}
          required={required}
          aria-label={showLabel ? undefined : label}
          aria-describedby={described}
          onChange={e => onChange(e.target.value as V)}
        >
          {groupOptions(options).map((run, i) => {
            const items = run.items.map(o => <option key={o.value} value={o.value} disabled={o.disabled}>{formatOption(o)}</option>);
            return run.group === null ? items : <optgroup key={`${run.group}-${i}`} label={run.group}>{items}</optgroup>;
          })}
        </select>
        <svg className="ui-select-chevron" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M4 6l4 4 4-4" /></svg>
      </div>
      {reason && <span id={reasonId} className="ui-hint">{reason}</span>}
    </div>
  );
}
