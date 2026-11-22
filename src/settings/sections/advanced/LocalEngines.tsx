import { useUiLocale } from '../../../i18n';
import { cancelModelPull } from '../../../lib/commands';
import { useLocalEngines, type BackendTab } from '../../../components/LocalAiSetup';
import Button from '../../../ui/Button';
import Segmented from '../../../ui/Segmented';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import type { LocalEndpoints } from '../../hooks/useLocalEndpoints';
import { useRow } from './useRow';

interface LocalEnginesProps {
  endpoints: LocalEndpoints;
  onAddToChain(endpointId: string, modelId: string): unknown;
  chainBusy: boolean;
}

/**
 * Advanced > Local AI: this Mac, the backend's prerequisites, Ollama's
 * recommended models, and a vLLM or generic server. The steps and commands
 * are LocalAiSetup's (useLocalEngines); this lays them out as rows.
 */
export default function LocalEngines({ endpoints, onAddToChain, chainBusy }: LocalEnginesProps) {
  const { text } = useUiLocale();
  const row = useRow();
  const engines = useLocalEngines({ endpoints: endpoints.endpoints, onEndpointAdded: endpoints.added, onModelsLoaded: endpoints.modelsLoaded });
  const { system, tab, checks, models, pulling } = engines;
  const ollama = endpoints.endpoints.find(e => e.backend_type === 'ollama');
  const tabs: { value: BackendTab; label: string }[] = [{ value: 'ollama', label: 'Ollama' }, { value: 'vllm', label: 'vLLM' }, { value: 'generic', label: text('Generic', '通用') }];
  const url = tab === 'vllm' ? engines.vllmUrl : engines.genericUrl;

  return (
    <SettingGroup title={text('Engines on this Mac', '此 Mac 上的引擎')}>
      <SettingRow {...row('local-system')} control={null}>
        <p className="ui-hint" role="status">{system
          ? `${system.total_ram_gb.toFixed(0)} GB RAM · ${engines.gpuLabel} · ${text(`Suggested largest model: ${system.recommended_max_params}`, `建議模型大小上限：${system.recommended_max_params}`)}`
          : text('Checking this Mac…', '正在檢查此 Mac…')}</p>
      </SettingRow>

      <SettingRow {...row('local-backend')} control={<Segmented<BackendTab> value={tab} options={tabs} label={row('local-backend').title} onChange={engines.setTab} />}>
        {checks.length > 0 && <ul className="settings-check-list">
          {checks.map((check, i) => (
            <li key={i}>
              <span className={`settings-chip${check.passed ? ' is-on' : ''}`}>{check.passed ? text('Ready', '已就緒') : text('Missing', '未就緒')}</span> {check.name}
              {!check.passed && check.fix_command && <span className="settings-row-actions settings-row-actions-start">
                <code className="settings-code">{check.fix_command}</code>
                <Button kind="quiet" onClick={() => navigator.clipboard.writeText(check.fix_command!)}>{text('Copy', '複製')}</Button>
              </span>}
              {!check.passed && !check.fix_command && check.fix_label && <span className="ui-hint"> {check.fix_label}</span>}
            </li>
          ))}
        </ul>}
        {checks.length === 0 && !engines.checkingPrereqs && <p className="ui-hint">{text('Choose Check again to see what is installed.', '按「重新檢查」查看已安裝的項目。')}</p>}
        <Button kind="quiet" busy={engines.checkingPrereqs} onClick={engines.refreshPrereqs}>{engines.checkingPrereqs ? text('Checking…', '檢查中…') : text('Check again', '重新檢查')}</Button>
      </SettingRow>

      <SettingRow {...row('local-models')} control={null}>
        {tab !== 'ollama'
          ? <p className="ui-hint">{text('Recommended models are for Ollama.', '建議模型適用於 Ollama。')}</p>
          : models.length === 0
            ? <p className="ui-hint">{text('No recommended models yet. Start Ollama, then check again.', '尚未有建議模型。請先啟動 Ollama，然後重新檢查。')}</p>
            : <ul className="settings-model-list">
              {models.map(m => (
                <li key={m.id}>
                  <span>{m.name} · {m.download_size_gb} GB{m.tool_capable ? ` · ${text('tools', '工具')}` : ''}<span className="ui-hint"> {m.description}</span></span>
                  <span className="settings-row-actions">
                    {m.already_pulled ? <>
                      <span className="settings-chip is-on">{text('Downloaded', '已下載')}</span>
                      <Button kind="secondary" disabled={!ollama || chainBusy} onClick={() => ollama && onAddToChain(ollama.id, m.id)}>{text('Add to priority', '加入優先次序')}</Button>
                    </> : <Button kind="secondary" disabled={pulling !== null} onClick={() => engines.pull(m.id)}>{pulling?.model === m.id ? `${pulling.percent}%` : text('Download', '下載')}</Button>}
                  </span>
                </li>
              ))}
            </ul>}
        {pulling && <div className="settings-row-actions settings-row-actions-start" role="status">
          <span>{text(`Downloading ${pulling.model}: ${pulling.percent}%`, `正在下載 ${pulling.model}：${pulling.percent}%`)}</span>
          <progress max={100} value={pulling.percent} />
          <Button kind="quiet" onClick={() => cancelModelPull(pulling.jobId)}>{text('Cancel', '取消')}</Button>
        </div>}
      </SettingRow>

      <SettingRow {...row('local-endpoint-url')} control={null}>
        {tab === 'ollama'
          ? <p className="ui-hint">{text('Choose vLLM or Generic above to connect a server.', '請在上方選擇 vLLM 或通用，以連接伺服器。')}</p>
          : <span className="settings-row-actions settings-row-actions-start">
            <input className="ui-input settings-url-input" aria-label={row('local-endpoint-url').title} value={url} spellCheck={false}
              placeholder={tab === 'vllm' ? 'http://localhost:8000' : 'http://localhost:8080'}
              onChange={e => (tab === 'vllm' ? engines.setVllmUrl(e.target.value) : engines.setGenericUrl(e.target.value))} />
            <Button kind="secondary" busy={engines.connectBusy} disabled={!url.trim()} onClick={() => engines.connect(tab)}>{text('Connect', '連接')}</Button>
          </span>}
        {engines.error && <p className="ui-secret-error" role="alert">{text('That did not work: ', '未能完成：')}{engines.error}</p>}
      </SettingRow>
    </SettingGroup>
  );
}
