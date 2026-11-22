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
  /** The save button's label; "Save" when not given. */
  saveLabel?: string;
  /** The line shown when `onSave` rejects; "Could not save. Try again." when not given. */
  errorFor?(e: unknown): string;
  /** The Remove confirmation: what is lost and whether it can be undone. A generic line when not given. */
  removeConfirm?: string;
  /** Keeps Save (and Enter) off, for example until a consent tick is on. */
  saveDisabled?: boolean;
}

/** Takes a secret once and hands it to `onSave`. It never receives the saved value back. */
export default function SecretField({ saved, onSave, onRemove, label, placeholder, saveLabel, errorFor, removeConfirm, saveDisabled }: SecretFieldProps) {
  const { text } = useUiLocale();
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<string | boolean>(false);
  // A saved key is never shown; Replace opens an empty field for a new one.
  const [replacing, setReplacing] = useState(false);

  async function save() {
    const secret = draft.trim();
    if (!secret || busy || saveDisabled) return;
    setBusy(true);
    setFailed(false);
    try {
      await onSave(secret);
      setDraft('');
      setReplacing(false);
    } catch (e) {
      setFailed(errorFor ? errorFor(e) : true);
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    setFailed(false);
    try {
      await onRemove?.();
    } catch {
      setFailed(text('Could not remove the key. Try again.', '無法移除密鑰，請再試一次。'));
    }
  }

  const failure = failed && <span className="ui-secret-error" role="alert">{failed === true ? text('Could not save. Try again.', '無法儲存，請再試一次。') : failed}</span>;

  if (saved && !replacing) {
    return (
      <div className="ui-field">
        <div className="ui-secret">
          <span className="ui-secret-saved">{text('Saved in Keychain', '已儲存在鑰匙圈')}</span>
          <Button kind="quiet" onClick={() => { setFailed(false); setReplacing(true); }}>{text('Replace', '更換')}</Button>
          {onRemove && (
            <Button
              kind="danger"
              confirm={{ message: removeConfirm ?? text('Remove this key from Keychain?', '要從鑰匙圈移除此金鑰嗎？'), label: text('Remove', '移除') }}
              onClick={remove}
            >
              {text('Remove', '移除')}
            </Button>
          )}
        </div>
        {failure}
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
        <Button kind="secondary" busy={busy} disabled={saveDisabled || !draft.trim()} onClick={save}>{saveLabel ?? text('Save', '儲存')}</Button>
        {replacing && <Button kind="quiet" disabled={busy} onClick={() => { setDraft(''); setFailed(false); setReplacing(false); }}>{text('Cancel', '取消')}</Button>}
      </div>
      {failure}
    </div>
  );
}
