import { useEffect, useId, useRef, useState } from 'react';
import './ui.css';

interface TextFieldProps {
  value: string;
  onCommit(v: string): unknown;
  label: string;
  placeholder?: string;
  maxLength?: number;
  showLabel?: boolean;
}

export default function TextField({ value, onCommit, label, placeholder, maxLength, showLabel }: TextFieldProps) {
  const id = useId();
  const [draft, setDraft] = useState(value);
  // The last value handed to onCommit, so Enter then blur cannot commit twice while `value` is still stale.
  const committed = useRef(value);
  useEffect(() => { setDraft(value); committed.current = value; }, [value]);

  function commit() {
    if (draft === committed.current) return;
    committed.current = draft;
    Promise.resolve(onCommit(draft)).catch(e => {
      committed.current = value;
      console.error(e);
    });
  }

  return (
    <div className="ui-field">
      {showLabel && <label className="ui-label" htmlFor={id}>{label}</label>}
      <input
        id={id}
        className="ui-input"
        type="text"
        value={draft}
        placeholder={placeholder}
        maxLength={maxLength}
        aria-label={showLabel ? undefined : label}
        onChange={e => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) commit(); }}
      />
    </div>
  );
}
