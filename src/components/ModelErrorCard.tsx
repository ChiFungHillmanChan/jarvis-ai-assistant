import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useUiLocale } from '../i18n';
import { offeredModels, offersPlainResend, pickAction, type Alternative, type Failure, type Pick } from '../lib/ai-error';

/**
 * A request that did not get an answer (P06): the model, why, and the
 * models that can run it with the keys saved here. Nothing is switched on
 * its own: picking one runs this request on it once (`onRetry`), and leaves
 * the saved choice as it is. The replacement for a disabled model is saved
 * first, as the disabled-model notice does. After an action ran (a calendar
 * event, a draft) no model is offered, because the request is never sent
 * again.
 */
export default function ModelErrorCard({ failure, onRetry, onPlainResend }: { failure: Failure; onRetry: (pick: Pick | null) => Promise<void> | void; onPlainResend?: () => Promise<void> | void }) {
  const { text } = useUiLocale();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const offered = offeredModels(failure);

  async function choose(choice: Alternative) {
    if (busy) return;
    setBusy(true); setError('');
    try {
      const action = pickAction(failure, choice);
      if (action.save) await invoke('confirm_model_replacement', action.save);
      await onRetry(action.pick);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function plain() {
    if (busy || !onPlainResend) return;
    setBusy(true); setError('');
    try { await onPlainResend(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }

  return <div className="ai-card is-error" role="alert">
    <p><strong>{failure.model.label}</strong></p>
    <p>{failure.message}</p>
    {failure.provider_reason && <p><small>{text('The service said: ', '服務回覆：')}{failure.provider_reason}</small></p>}
    {onPlainResend && offersPlainResend(failure) && <div className="tool-approval-actions"><button type="button" className="update-toast-action" disabled={busy} onClick={() => void plain()}>{text('Send again without Think harder', '不使用深入思考再次傳送')}</button></div>}
    {failure.actions_ran > 0
      ? <p>{text('Actions already carried out are kept. JARVIS does not send this request again, so nothing is done twice. Check the result before asking again.', '已完成的操作會保留。為免重複執行，JARVIS 不會再次傳送這個請求。請先檢查結果，再重新提出要求。')}</p>
      : offered.length > 0
        ? <><p>{text('Run this request on another model (your saved choice stays as it is):', '改用其他模型執行這個請求（已儲存的選擇不會更改）：')}</p>
          <div className="ai-card-picks">{offered.map(choice => <button key={`${choice.pick.provider_type}|${choice.pick.base_url ?? ''}|${choice.pick.endpoint_id ?? ''}|${choice.pick.model_id ?? ''}`} type="button" className="update-toast-action" disabled={busy} onClick={() => void choose(choice)}>{choice.saves_at !== null ? text(`Use ${choice.label} from now on`, `之後改用 ${choice.label}`) : choice.label}</button>)}</div></>
        : <p>{text('No other model with a saved key can run this request. Add a key in Settings → Your AI.', '沒有其他已儲存密鑰的模型可以執行這個請求。請在「設定 → 你的 AI」加入密鑰。')}</p>}
    {error && <p className="setup-error">{error}</p>}
  </div>;
}
