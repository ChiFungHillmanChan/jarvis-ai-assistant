import { useUiLocale } from '../../../i18n';
import type { AiModelCatalog } from '../../../lib/ai-models';
import { addModelLabel } from '../../../lib/ai-catalog';
import type { BedrockListState } from '../../../lib/bedrock-models';
import type { ProviderChainEntry } from '../../../lib/types';
import BedrockModelPicker from '../../../components/BedrockModelPicker';
import Button from '../../../ui/Button';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import Switch from '../../../ui/Switch';
import { addEntry, chainLabel, moveEntry, removeEntry, setEntryModel, toggleEntry } from '../../advanced-chain';
import type { ChainEditor } from '../../hooks/useChainEditor';
import { useRow } from './useRow';

/** The picker's inline looks belong to the old page; the dialog styles it with settings.css. */
const PLAIN = { select: {}, button: {}, input: {} };

interface ProviderOrderProps {
  editor: ChainEditor;
  catalog: AiModelCatalog;
  bedrock: { list: BedrockListState; reload: () => void };
}

/**
 * Advanced > Models: the saved provider order. Each edit saves at once through
 * update_provider_chain (useChainEditor); Remove asks first.
 */
export default function ProviderOrder({ editor, catalog, bedrock }: ProviderOrderProps) {
  const { text } = useUiLocale();
  const row = useRow();
  const { chain, save, saveError, busy } = editor;
  const add = (entry: Omit<ProviderChainEntry, 'position' | 'enabled'>) => save(addEntry(chain, entry));
  const hasBedrock = chain.some(entry => entry.provider_type === 'bedrock');
  const { defaults } = catalog;

  return (
    <SettingGroup title={text('Provider order', '供應商次序')}>
      <SettingRow {...row('chain-order')} control={null}>
        <p className="ui-hint">{text('JARVIS uses the first enabled provider, and only that one. When it cannot answer, the error card offers the other services that have a saved key; JARVIS never switches on its own.', 'JARVIS 只會使用第一個已啟用的供應商。它未能回覆時，錯誤卡會列出其他已儲存金鑰的服務供你選擇；JARVIS 不會自行更換。')}</p>
        {chain.length === 0
          ? <p className="ui-hint" role="status">{text('No order is saved. JARVIS uses the default provider below.', '尚未儲存次序。JARVIS 會使用下方的預設供應商。')}</p>
          : <ol className="settings-chain-list">
            {chain.map((entry, i) => {
              const label = chainLabel(entry, catalog, text);
              return (
                <li key={`${i}-${entry.provider_type}-${entry.endpoint_id ?? ''}-${entry.model_id ?? ''}`} className="settings-chain-item">
                  <span className="settings-chain-label" data-off={!entry.enabled || undefined}>{`${i + 1}. ${label}`}</span>
                  <span className="settings-row-actions">
                    <Button kind="quiet" disabled={busy || i === 0} onClick={() => save(moveEntry(chain, i, -1))}>{text('Move up', '上移')}</Button>
                    <Button kind="quiet" disabled={busy || i === chain.length - 1} onClick={() => save(moveEntry(chain, i, 1))}>{text('Move down', '下移')}</Button>
                    <Switch checked={entry.enabled} disabled={busy} label={text(`Use ${label}`, `使用 ${label}`)} onChange={() => { void save(toggleEntry(chain, i)); }} />
                    <Button kind="danger" disabled={busy}
                      confirm={{ message: text(`Remove ${label} from the provider order? JARVIS stops using it. You can add it again later.`, `要從供應商次序移除 ${label} 嗎？JARVIS 將不再使用它。你之後可以再次加入。`), label: text('Remove', '移除') }}
                      onClick={() => save(removeEntry(chain, i))}>{text('Remove', '移除')}</Button>
                  </span>
                </li>
              );
            })}
          </ol>}
        {saveError && <p className="ui-secret-error" role="alert">{text('Could not save the provider order.', '無法儲存供應商次序。')} {saveError}</p>}
      </SettingRow>

      <SettingRow {...row('chain-add')} control={null}>
        <span className="settings-row-actions settings-row-actions-start">
          {defaults.gemini_pro && <Button kind="secondary" disabled={busy} onClick={() => add({ provider_type: 'gemini', model_id: defaults.gemini_pro })}>{addModelLabel(catalog, defaults.gemini_pro)}</Button>}
          <Button kind="secondary" disabled={busy} onClick={() => add({ provider_type: 'gemini', model_id: defaults.gemini_flash })}>+ Gemini</Button>
          <Button kind="secondary" disabled={busy} onClick={() => add({ provider_type: 'claude' })}>+ Claude</Button>
          <Button kind="secondary" disabled={busy} onClick={() => add({ provider_type: 'openai' })}>+ OpenAI</Button>
          <Button kind="secondary" disabled={busy} onClick={() => add({ provider_type: 'bedrock', model_id: defaults.bedrock || undefined })}>+ Bedrock</Button>
        </span>
      </SettingRow>

      <SettingRow {...row('chain-bedrock-model')} control={null}>
        {!hasBedrock && <p className="ui-hint">{text('Add Bedrock to the order to choose its model. AWS keys and region are in Cloud providers.', '將 Bedrock 加入次序後即可選擇其模型。AWS 金鑰及地區在「雲端服務」中設定。')}</p>}
        {chain.map((entry, i) => entry.provider_type === 'bedrock' && (
          <BedrockModelPicker key={i} place="settings" saved={entry.model_id ?? ''} fallback={defaults.bedrock} listState={bedrock.list} onReload={bedrock.reload}
            onSave={model => save(setEntryModel(chain, i, model), true)} styles={PLAIN} />
        ))}
      </SettingRow>
    </SettingGroup>
  );
}
