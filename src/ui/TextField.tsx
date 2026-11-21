import { useEffect, useId, useState } from 'react';
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
  useEffect(() => { setDraft(value); }, [value]);

  function commit() {
    if (draft !== value) void onCommit(draft);
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
        onKeyDown={e => { if (e.key === 'Enter') commit(); }}
      />
    </div>
  );
}
