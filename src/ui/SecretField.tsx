import { useState } from 'react';
import { useUiLocale } from '../i18n';
import Button from './Button';
import './ui.css';

interface SecretFieldProps {
  saved: boolean;
  onSave(v: string): Promise<void>;
  onRemove?: () => Promise<void>;
  label: string;
  placeholder?: string;
}

/** Takes a secret once and hands it to `onSave`. It never receives the saved value back. */
export default function SecretField({ saved, onSave, onRemove, label, placeholder }: SecretFieldProps) {
  const { text } = useUiLocale();
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  async function save() {
    const secret = draft.trim();
    if (!secret || busy) return;
    setBusy(true);
    setFailed(false);
    try {
      await onSave(secret);
      setDraft('');
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  }

  if (saved) {
    return (
      <div className="ui-secret">
        <span className="ui-secret-saved">{text('Saved in Keychain', '已儲存在鑰匙圈')}</span>
        {onRemove && (
          <Button
            kind="danger"
            confirm={{ message: text('Remove this key from Keychain?', '要從鑰匙圈移除此金鑰嗎？'), label: text('Remove', '移除') }}
            onClick={onRemove}
          >
            {text('Remove', '移除')}
          </Button>
        )}
      </div>
    );
  }

  return (
    <div className="ui-field">
      <div className="ui-secret">
        <input
          className="ui-input"
          type="password"
          value={draft}
          placeholder={placeholder}
          aria-label={label}
          autoComplete="off"
          spellCheck={false}
          onChange={e => setDraft(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') void save(); }}
        />
        <Button kind="secondary" busy={busy} disabled={!draft.trim()} onClick={save}>{text('Save', '儲存')}</Button>
      </div>
      {failed && <span className="ui-secret-error" role="alert">{text('Could not save. Try again.', '無法儲存，請再試一次。')}</span>}
    </div>
  );
}
