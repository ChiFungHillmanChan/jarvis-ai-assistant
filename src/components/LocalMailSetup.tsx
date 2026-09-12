import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useUiLocale } from '../i18n';
import { addLocalEndpoint, cancelModelPull, listLocalEndpoints, openUrl, pullModel } from '../lib/commands';
import { mailWorkspace } from '../lib/mail';
import type { ModelPullProgress } from '../lib/types';
import { followJob } from '../lib/setup-flow';
import { applyTestedMailSettings, cancelLocalSetup, prepareLocalAi, probeLocalSetup, setupError, testMailAiConnection, type LocalSetupProbe, type LocalSetupProgress } from '../lib/setup';

const choices = [
  { tag: 'qwen3.5:2b', size: 2.7, ram: 8 },
  { tag: 'qwen3.5:4b', size: 3.4, ram: 16 },
  { tag: 'qwen3.5:9b', size: 6.6, ram: 24 },
];

export default function LocalMailSetup({ onReady, onBusyChange }: { onReady?: () => void | Promise<void>; onBusyChange?: (busy: boolean) => void }) {
  const { text } = useUiLocale();
  const [probe, setProbe] = useState<LocalSetupProbe | null>(null);
  const [model, setModel] = useState('');
  const [stage, setStage] = useState('');
  const [bytes, setBytes] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState('');
  const [ready, setReady] = useState(false);
  const [checking, setChecking] = useState(false);
  const operation = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; void refresh(); return () => { mounted.current = false; operation.current?.abort(); }; }, []);

  async function refresh() {
    setChecking(true); setError('');
    try {
      const value = await probeLocalSetup();
      if (mounted.current) { setProbe(value); setModel(previous => previous || value.recommendedTag || ''); }
    } catch { if (mounted.current) setError(text('Could not check this Mac. Retry the check.', '未能檢查呢部 Mac，請重新檢查。')); }
    finally { if (mounted.current) setChecking(false); }
  }

  async function prepare() {
    if (operation.current || !model) return;
    const controller = new AbortController(); operation.current = controller; onBusyChange?.(true);
    const { signal } = controller;
    const check = () => { if (signal.aborted || !mounted.current) throw Error('cancelled'); };
    setError(''); setReady(false); setBytes(null); setStage('checking');
    try {
      const previous = (await mailWorkspace()).settings;
      check();
      const startId = crypto.randomUUID();
      const started = await followJob<LocalSetupProgress>({
        jobId: startId, signal,
        subscribe: callback => listen<LocalSetupProgress>('local-setup-progress', event => callback(event.payload)),
        start: () => prepareLocalAi(model, startId),
        cancel: cancelLocalSetup,
        terminal: event => ['server_ready', 'needs_install', 'failed', 'cancelled'].includes(event.stage),
        progress: event => { if (!signal.aborted && mounted.current) setStage(event.stage); },
      });
      check();
      if (started.stage !== 'server_ready') throw Error(started.errorCode || 'server_not_ready');
      const refreshed = await probeLocalSetup(); check();
      setProbe(refreshed);
      if (!refreshed.installedModels.includes(model)) {
        const endpoints = await listLocalEndpoints(); check();
        let endpoint = endpoints.find(item => item.backend_type === 'ollama' && ['http://localhost:11434', 'http://127.0.0.1:11434'].includes(item.url.replace(/\/$/, '')));
        if (!endpoint) endpoint = await addLocalEndpoint('Ollama', 'http://127.0.0.1:11434', 'ollama');
        check();
        const pullId = crypto.randomUUID();
        const downloaded = await followJob<ModelPullProgress>({
          jobId: pullId, signal,
          subscribe: callback => listen<ModelPullProgress>('model-pull-progress', event => callback(event.payload)),
          start: () => pullModel(endpoint!.id, model, pullId), cancel: cancelModelPull,
          terminal: event => event.status === 'complete' || event.status === 'error',
          progress: event => { if (!signal.aborted && mounted.current) { setStage(event.status); setBytes(event.total_bytes > 0 ? { done: event.completed_bytes, total: event.total_bytes } : null); } },
        });
        check();
        if (downloaded.status !== 'complete') throw Error('download_failed');
      }
      setStage('testing'); setBytes(null);
      const candidate = { ...previous, processing_mode: 'local', local_model: model };
      const tested = await testMailAiConnection(candidate); check();
      if (tested.state !== 'ready') throw Error(tested.errorCode || 'probe_failed');
      setStage('applying');
      await applyTestedMailSettings(candidate, previous);
      if (!mounted.current) return;
      setReady(true); setStage('');
      try { await onReady?.(); } catch { setError(text('The model is saved for mail, but the page could not refresh. Reopen setup to check it.', '模型已儲存畀電郵使用，但畫面未能更新。請重開設定核對。')); }
    } catch (reason) {
      if (mounted.current) {
        const code = reason instanceof Error ? reason.message : String(reason);
        setError(code === 'settings_changed' ? text('Mail settings changed while this was running. The model is downloaded; review your settings and retry.', '設定期間電郵設定有改動。模型已下載，請核對設定再試。') : setupError(code, text));
      }
    } finally { if (operation.current === controller) operation.current = null; if (mounted.current) { setStage(''); onBusyChange?.(false); } }
  }

  const busy = !!stage;
  const labels: Record<string, string> = {
    checking: text('Checking this Mac…', '檢查緊呢部 Mac…'), starting: text('Starting Ollama…', '啟動緊 Ollama…'),
    server_ready: text('Engine started; preparing model…', '引擎已啟動，準備緊模型…'),
    downloading: text('Downloading model…', '下載緊模型…'), verifying: text('Verifying download…', '核對緊下載…'),
    complete: text('Download complete; preparing test…', '下載完成，準備測試…'),
    testing: text('Testing with synthetic Cantonese mail…', '用合成廣東話郵件測試緊…'), applying: text('Applying to mail…', '套用到電郵工作區…'),
  };
  return <section className="setup-local" aria-label={text('Local AI for mail', '電郵本機 AI')}>
    <h3>{text('Local AI, without an API key', '本機 AI，唔使 API key')}</h3>
    <p>{text('Prepare Ollama, download a model, test it, then use it for mail. First installation needs your confirmation. Existing mail settings change only after a successful test.', '準備 Ollama、下載模型、完成測試，再套用到電郵。首次安裝需要你確認；測試成功後先更改現有電郵設定。')}</p>
    {probe && <p>{probe.os} {probe.macosVersion} · {probe.totalRamGb.toFixed(0)} GB RAM · {probe.availableDiskGb === null ? text('Free disk unknown', '未能取得磁碟餘額') : `${probe.availableDiskGb.toFixed(1)} GB ${text('free disk', '磁碟餘額')}`}</p>}
    <label>{text('Model to download', '要下載嘅模型')}<select value={model} disabled={busy} onChange={event => { setModel(event.target.value); setReady(false); }}>
      <option value="">{text('Choose a model', '揀模型')}</option>
      {choices.map(item => <option key={item.tag} value={item.tag} disabled={probe ? probe.totalRamGb < item.ram : false}>{item.tag} · {item.size} GB · {item.ram} GB RAM {text('suggested', '建議')}{probe?.recommendedTag === item.tag ? text(' · recommended', ' · 推薦') : ''}</option>)}
    </select></label>
    <p className="setup-hint">{text('No cloud token fees; disk space, electricity and performance depend on your Mac. Small models may fail mail citation checks. If citations are invalid or content is too long, analysis stops and you can choose another model; mail will not switch to cloud.', '冇雲端 token 費；磁碟、電費同速度視乎你部 Mac。細模型可能未能提供有效郵件引用；引用或容量檢查失敗會停止分析，你可以另揀模型，唔會轉用雲端。')}</p>
    <div className="setup-actions">
      <button type="button" disabled={busy || checking} onClick={() => void refresh()}>{checking ? text('Checking…', '檢查中…') : text('Check again', '重新檢查')}</button>
      {probe && !probe.ollamaInstalled && !probe.ollamaRunning && <button type="button" disabled={busy} onClick={() => void openUrl('https://ollama.com/download/mac').catch(() => setError(text('Could not open the official download page.', '未能開啟官方下載頁。')))}>{text('Install Ollama', '安裝 Ollama')}</button>}
      <button type="button" className="setup-primary" disabled={busy || checking || !probe || !model} onClick={() => void prepare()}>{text('Prepare and use for mail', '準備並用於電郵')}</button>
      {busy && <button type="button" disabled={stage === 'applying'} onClick={() => operation.current?.abort()}>{text('Cancel', '取消')}</button>}
    </div>
    {busy && <div role="status" aria-live="polite"><p>{labels[stage] || text('Preparing…', '準備中…')}</p>{bytes && <><progress max={bytes.total} value={Math.min(bytes.done, bytes.total)} /><span> {(bytes.done / 1e9).toFixed(2)} / {(bytes.total / 1e9).toFixed(2)} GB</span></>}</div>}
    {ready && <p role="status">{text('The sample passed. Mail now uses this local model. Assistant and voice settings have their own setup.', '示範測試通過，電郵已改用呢個本機模型。AI 助理同語音有各自嘅設定。')}</p>}
    {error && <p role="alert" className="setup-error">{error}</p>}
  </section>;
}
