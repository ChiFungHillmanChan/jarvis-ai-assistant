import { useUiLocale } from '../../../i18n';
import LocalMailSetup from '../../../components/LocalMailSetup';
import Button from '../../../ui/Button';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import { addEntry } from '../../advanced-chain';
import { useChainEditor } from '../../hooks/useChainEditor';
import { useLocalEndpoints } from '../../hooks/useLocalEndpoints';
import LocalEngines from './LocalEngines';
import LocalEndpoints from './LocalEndpoints';
import { useRow } from './useRow';

/**
 * Settings > Advanced > Local AI: engines, endpoints, and the one place the
 * local mail setup appears. "Prepare and use for mail" replaces the mail
 * processing mode, so it asks first.
 */
export default function LocalAi() {
  const { text } = useUiLocale();
  const row = useRow();
  const endpoints = useLocalEndpoints();
  const editor = useChainEditor();
  const addToChain = (endpointId: string, modelId: string) => editor.save(addEntry(editor.chain, { provider_type: 'local', endpoint_id: endpointId, model_id: modelId }));

  if (endpoints.loadError || editor.loadError) {
    return (
      <p className="ui-secret-error" role="alert">
        {text('Could not load the local AI settings.', '無法載入本機 AI 設定。')}{' '}
        <Button kind="quiet" onClick={() => { void endpoints.load(); void editor.reload(); }}>{text('Retry', '重試')}</Button>
      </p>
    );
  }
  if (!endpoints.ready || !editor.ready) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  return <>
    <p className="ui-hint">{text('Optional. Connect a local server or download a model only when you want AI to run on this Mac.', '此為選用設定。只在需要於此 Mac 上執行 AI 時，才連接本機伺服器或下載模型。')}</p>
    <LocalEngines endpoints={endpoints} onAddToChain={addToChain} chainBusy={editor.busy} />
    <LocalEndpoints endpoints={endpoints} editor={editor} onAddToChain={addToChain} />
    {editor.saveError && <p className="ui-secret-error" role="alert">{text('Could not save the provider order.', '無法儲存供應商次序。')} {editor.saveError}</p>}

    <SettingGroup title={text('Mail on this Mac', '在此 Mac 上處理郵件')}>
      <SettingRow {...row('local-mail-setup')} control={null}>
        <LocalMailSetup confirmPrepare={{ message: text('Use a model on this Mac for mail? Once the test passes, mail analysis switches from its current AI to this model. You can switch it back again in the mail settings.', '要使用此 Mac 上的模型處理郵件嗎？測試通過後，郵件分析會由目前的 AI 改用此模型。你可在郵件設定中再次改回。'), label: text('Prepare and use', '準備並使用') }} onReady={() => endpoints.load()} />
      </SettingRow>
    </SettingGroup>
  </>;
}
