import { useState } from 'react';
import { useUiLocale } from '../../../i18n';
import Button from '../../../ui/Button';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import { withoutEndpoint } from '../../advanced-chain';
import type { ChainEditor } from '../../hooks/useChainEditor';
import type { LocalEndpoints as Endpoints } from '../../hooks/useLocalEndpoints';
import { useRow } from './useRow';

interface LocalEndpointsProps {
  endpoints: Endpoints;
  editor: ChainEditor;
  onAddToChain(endpointId: string, modelId: string): unknown;
}

/**
 * Advanced > Local AI: the saved local model endpoints, each with Test,
 * Add to priority for its models and Remove (which also takes its rows out of
 * the provider order), and a form to add one.
 */
export default function LocalEndpoints({ endpoints, editor, onAddToChain }: LocalEndpointsProps) {
  const { text } = useUiLocale();
  const row = useRow();
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const tools = (kind: string) => kind === 'native' ? text('tools', '工具') : kind === 'prompt_injected' ? text('prompt tools', '提示工具') : text('chat only, cannot use tools', '只可對話，不能使用工具');

  return (
    <SettingGroup title={text('Endpoints', '端點')}>
      <SettingRow {...row('local-endpoints')} control={null}>
        {endpoints.endpoints.length === 0 && <p className="ui-hint">{text('No endpoints yet. Add one below, or download a model above.', '尚未有端點。請在下方新增，或在上方下載模型。')}</p>}
        <ul className="settings-endpoint-list">
          {endpoints.endpoints.map(ep => {
            const health = endpoints.health[ep.id];
            const models = endpoints.models[ep.id] ?? [];
            return (
              <li key={ep.id} className="settings-endpoint">
                <div className="settings-row-actions settings-row-actions-start">
                  <strong>{ep.name}</strong>
                  <span className="settings-chip">{ep.backend_type}</span>
                  <span className={`settings-chip${ep.last_health_status ? ' is-on' : ''}`}>{ep.last_health_status ? text('Reachable', '可連接') : text('Not reached yet', '尚未連接')}</span>
                </div>
                <code className="settings-code">{ep.url}</code>
                {health && <p className="ui-hint" role="status">{health.reachable
                  ? text(`${health.model_count} models, ${health.latency_ms} ms`, `${health.model_count} 個模型，${health.latency_ms} 毫秒`)
                  : text('Unreachable', '無法連接')}</p>}
                {models.length > 0 && <ul className="settings-model-list">
                  {models.map(m => (
                    <li key={m.id}>
                      <code className="settings-code">{m.id}</code>
                      <span className="settings-row-actions">
                        <span className="ui-hint">{tools(m.supports_tools)}</span>
                        <Button kind="secondary" disabled={editor.busy} onClick={() => onAddToChain(ep.id, m.id)}>{text('Add to priority', '加入優先次序')}</Button>
                      </span>
                    </li>
                  ))}
                </ul>}
                <span className="settings-row-actions settings-row-actions-start">
                  <Button kind="secondary" onClick={() => endpoints.test(ep.id)}>{text('Test connection', '測試連接')}</Button>
                  <Button kind="danger" disabled={editor.busy}
                    confirm={{ message: text(`Remove the endpoint ${ep.name}? Its models leave the provider order and any API key saved for it is deleted. This cannot be undone.`, `要移除端點 ${ep.name} 嗎？其模型會從供應商次序中移除，為它儲存的 API 金鑰亦會刪除。此操作無法復原。`), label: text('Remove', '移除') }}
                    onClick={async () => {
                      await endpoints.remove(ep.id);
                      const left = withoutEndpoint(editor.chain, ep.id);
                      if (left.length !== editor.chain.length) await editor.save(left);
                    }}>
                    {text('Remove', '移除')}
                  </Button>
                </span>
              </li>
            );
          })}
        </ul>
      </SettingRow>

      <SettingRow {...row('local-endpoint-add')} control={null}>
        <div className="settings-advanced-form">
          <input className="ui-input" aria-label={text('Endpoint name', '端點名稱')} value={name} placeholder={text('Name (optional)', '名稱（可選）')} onChange={e => setName(e.target.value)} />
          <span className="settings-row-actions settings-row-actions-start">
            <input className="ui-input settings-url-input" aria-label={text('Endpoint URL', '端點網址')} value={url} spellCheck={false} placeholder="http://localhost:11434" onChange={e => setUrl(e.target.value)} />
            <Button kind="secondary" disabled={!url.trim()} onClick={async () => { await endpoints.add(name, url); setName(''); setUrl(''); }}>{text('Add endpoint', '新增端點')}</Button>
          </span>
        </div>
        {endpoints.error && <p className="ui-secret-error" role="alert">{endpoints.error}</p>}
      </SettingRow>
    </SettingGroup>
  );
}
