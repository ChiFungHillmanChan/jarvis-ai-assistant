import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useUiLocale } from '../i18n';
import { choiceKey, noticesToShow, noticeText, placeOf, type DisabledChoice, type DisabledEvent } from '../lib/model-disabled';

const SEEN_KEY = 'jarvis.model-disabled.seen';

function readSeen(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(SEEN_KEY) ?? '[]') as string[]); } catch { return new Set(); }
}
function markSeen(choice: DisabledChoice) {
  const seen = readSeen();
  seen.add(choiceKey(choice));
  try { localStorage.setItem(SEEN_KEY, JSON.stringify([...seen])); } catch { /* Shown again next launch; nothing is switched either way. */ }
}

/**
 * A saved model the registry disabled: a blocking notice that names the
 * replacement and saves it only on Confirm. It opens once on its own, and
 * again whenever a job on that model stops (model-disabled): a background
 * job, or mail processing set to a model of its own. A request the user
 * waits on shows the error card instead (ModelErrorCard, P06), which
 * offers the same replacement.
 */
export default function ModelDisabledNotice() {
  const { text } = useUiLocale();
  const [open, setOpen] = useState<DisabledChoice[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState('');
  const live = useRef(true);

  useEffect(() => {
    live.current = true;
    const refresh = (event: DisabledEvent | null) => {
      void invoke<DisabledChoice[]>('disabled_models')
        .then(choices => { if (live.current) setOpen(previous => noticesToShow(previous, choices, readSeen(), event)); })
        .catch(() => {});
    };
    refresh(null);
    const stops = [
      listen<DisabledEvent>('model-disabled', event => refresh(event.payload)),
      listen('registry-updated', () => refresh(null)),
    ];
    return () => { live.current = false; stops.forEach(stop => void stop.then(unlisten => unlisten()).catch(() => {})); };
  }, []);

  const choice = open[0];
  if (!choice) return saved ? <div className="update-toast" role="status"><span>{saved}</span><button type="button" className="tool-approval-decline" onClick={() => setSaved('')}>{text('Close', '關閉')}</button></div> : null;

  function close(current: DisabledChoice) {
    markSeen(current);
    setError('');
    setOpen(previous => previous.filter(other => choiceKey(other) !== choiceKey(current)));
  }
  async function confirm(current: DisabledChoice) {
    if (busy) return;
    setBusy(true); setError('');
    try {
      const replacement = await invoke<string>('confirm_model_replacement', { place: placeOf(current), model: current.model });
      if (!live.current) return;
      close(current);
      setSaved(text(`Now using ${replacement}.`, `已改用 ${replacement}。`));
    } catch (e) {
      if (live.current) setError(String(e));
    } finally {
      if (live.current) setBusy(false);
    }
  }

  return <div className="confirm-backdrop">
    <div className="confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="model-disabled-title" aria-describedby="model-disabled-message">
      <p id="model-disabled-title"><strong>{text('A model you chose is no longer available', '你選擇的模型已停用')}</strong></p>
      <p id="model-disabled-message">{noticeText(choice, text)}</p>
      {error && <p className="setup-error" role="alert">{error}</p>}
      <div className="tool-approval-actions">
        {choice.replacement && <button type="button" className="update-toast-action" disabled={busy} onClick={() => void confirm(choice)}>{text(`Use ${choice.replacement}`, `改用 ${choice.replacement}`)}</button>}
        <button type="button" className="tool-approval-decline" disabled={busy} onClick={() => close(choice)}>{choice.replacement ? text('Not now', '稍後') : text('OK', '確定')}</button>
      </div>
    </div>
  </div>;
}
