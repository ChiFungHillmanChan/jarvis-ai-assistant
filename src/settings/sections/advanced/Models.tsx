import { useState } from 'react';
import { useUiLocale } from '../../../i18n';
import { useAiModels } from '../../../lib/ai-models';
import { localized } from '../../../lib/ai-catalog';
import { updateSetting } from '../../../lib/commands';
import { useBedrockModelList } from '../../../hooks/useBedrockModelList';
import Button from '../../../ui/Button';
import Select from '../../../ui/Select';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import { activePreset } from '../../advanced-chain';
import { useChainEditor } from '../../hooks/useChainEditor';
import { useSettingsValues } from '../../hooks/useSettingsValues';
import ProviderOrder from './ProviderOrder';
import ChooseModel from './ChooseModel';
import MailModels from './MailModels';
import { useRow } from './useRow';

/**
 * Settings > Advanced > Models: the provider order, the default provider,
 * any model or address, and the models mail analysis uses.
 *
 * Default provider (`ai_provider`) is reachable: with no provider order saved
 * (provider_chain empty: no AI chosen yet, an install from before Your AI, or
 * every row removed here) the router and the mail gate build the order from
 * this preset (ai::unsaved_chain, ai_choice::rebuilt_router,
 * mail_policy). It is saved with update_setting, which rebuilds the live router.
 */
export default function Models() {
  const { text } = useUiLocale();
  const row = useRow();
  const catalog = useAiModels();
  const editor = useChainEditor();
  const values = useSettingsValues();
  const [preset, setPreset] = useState<string | null>(null);
  const [presetFailed, setPresetFailed] = useState(false);
  // The Bedrock list is asked for while a Bedrock row is on screen, or once someone asks for it.
  const [asked, setAsked] = useState(false);
  const list = useBedrockModelList(asked || editor.chain.some(entry => entry.provider_type === 'bedrock'));
  const bedrock = { list: list.list, reload: () => { setAsked(true); list.reload(); } };

  if (editor.loadError || values.error) {
    return (
      <p className="ui-secret-error" role="alert">
        {text('Could not load the model settings.', '無法載入模型設定。')}{' '}
        <Button kind="quiet" onClick={() => { void editor.reload(); void values.reload(); }}>{text('Retry', '重試')}</Button>
      </p>
    );
  }
  if (!editor.ready || !values.ready || !catalog) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  const saved = preset ?? values.values.ai_provider ?? null;
  function choosePreset(next: string) {
    const previous = preset;
    setPreset(next);
    setPresetFailed(false);
    updateSetting('ai_provider', next).catch(e => { console.error(e); setPreset(previous); setPresetFailed(true); });
  }

  return <>
    <ProviderOrder editor={editor} catalog={catalog} bedrock={bedrock} />

    <SettingGroup title={text('When no order is saved', '未儲存次序時')}>
      <SettingRow {...row('default-provider')} control={
        catalog.presets.length > 0
          ? <Select value={activePreset(saved, catalog)} label={row('default-provider').title} width={260} onChange={choosePreset}
            options={catalog.presets.map(p => ({ value: p.id, label: localized(p.label, text) }))} />
          : null
      }>
        {editor.chain.length > 0 && <span className="ui-hint">{text('A provider order is saved, so this is not used now.', '已儲存供應商次序，因此目前不會使用此設定。')}</span>}
        {presetFailed && <span className="ui-secret-error" role="alert">{text('Could not save. Try again.', '無法儲存，請再試一次。')}</span>}
      </SettingRow>
    </SettingGroup>

    <ChooseModel editor={editor} catalog={catalog} />
    <MailModels catalog={catalog} bedrock={bedrock} />
  </>;
}
