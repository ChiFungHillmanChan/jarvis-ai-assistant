import { useRef, type KeyboardEvent } from 'react';
import { nextIndex } from './keys';
import './ui.css';

interface SegmentedProps<V extends string> {
  value: V;
  options: { value: V; label: string }[];
  onChange(v: V): void;
  label: string;
}

export default function Segmented<V extends string>({ value, options, onChange, label }: SegmentedProps<V>) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const selected = options.findIndex(o => o.value === value);
  const tabStop = selected === -1 ? 0 : selected;

  function onKeyDown(e: KeyboardEvent, index: number) {
    const next = nextIndex(index, e.key, options.length);
    if (next === null) return;
    e.preventDefault();
    onChange(options[next].value);
    buttons.current[next]?.focus();
  }

  return (
    <div className="ui-segmented" role="radiogroup" aria-label={label}>
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={el => { buttons.current[i] = el; }}
          type="button"
          role="radio"
          aria-checked={i === selected}
          tabIndex={i === tabStop ? 0 : -1}
          onClick={() => onChange(o.value)}
          onKeyDown={e => onKeyDown(e, i)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
