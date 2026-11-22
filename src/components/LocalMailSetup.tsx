import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useUiLocale } from '../i18n';
import { addLocalEndpoint, cancelModelPull, listLocalEndpoints, openUrl, pullModel } from '../lib/commands';
import { mailWorkspace } from '../lib/mail';
import type { ModelPullProgress } from '../lib/types';
import { followJob } from '../lib/setup-flow';
import { applyTestedMailSettings, cancelLocalSetup, prepareLocalAi, probeLocalSetup, setupError, testMailAiConnection, type LocalSetupProbe, type LocalSetupProgress } from '../lib/setup';
import { useAiModels } from '../lib/ai-models';
import { mailSetupModels } from '../lib/ai-catalog';
import Select from '../ui/Select';
import Button from '../ui/Button';

/**
 * Preparing Ollama and an approved model on this Mac: check the Mac, start
 * or install Ollama, download the model, test it on sample mail, then use it
 * for mail. Shared by this panel (onboarding) and Settings > Your AI.
 * With `applyToMail: false` it stops once the model is on this Mac and
 * leaves mail settings as they are. Unmounting cancels a run that is still
 * going, so Settings owns its runs above the dialog (settings/LocalSetupContext).
 * With `probeOnMount: false` the Mac is checked only when `refresh` is called.
 */
export function useLocalSetup({ onReady, onBusyChange, applyToMail = true, probeOnMount = true }: { onReady?: () => void | Promise<void>; onBusyChange?: (busy: boolean) => void; applyToMail?: boolean; probeOnMount?: boolean }) {
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
  useEffect(() => { mounted.current = true; if (probeOnMount) void refresh(); return () => { mounted.current = false; operation.current?.abort(); }; }, []);

  async function refresh() {
    setChecking(true); setError('');
    try {
      const value = await probeLocalSetup();
      if (mounted.current) { setProbe(value); setModel(previous => previous || value.recommendedTag || ''); }
    } catch { if (mounted.current) setError(text('Could not check this Mac. Retry the check.', '未能檢查此 Mac，請重試。')); }
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
      if (!applyToMail) {
        setReady(true); setStage(''); setBytes(null);
        try { await onReady?.(); } catch { /* the card's Check again reads the Mac again */ }
        return;
      }
      setStage('testing'); setBytes(null);
      const candidate = { ...previous, processing_mode: 'local', local_model: model };
      const tested = await testMailAiConnection(candidate); check();
      if (tested.state !== 'ready') throw Error(tested.errorCode || 'probe_failed');
      setStage('applying');
      await applyTestedMailSettings(candidate, previous);
      if (!mounted.current) return;
      setReady(true); setStage('');
      try { await onReady?.(); } catch { setError(text('The model is saved for mail, but the page could not refresh. Reopen setup to check it.', '模型已儲存供電郵使用，但畫面未能更新。請重新開啟設定以核對。')); }
    } catch (reason) {
      if (mounted.current) {
        const code = reason instanceof Error ? reason.message : String(reason);
        setError(code === 'settings_changed' ? text('Mail settings changed while this was running. The model is downloaded; review your settings and retry.', '電郵設定在此流程進行期間有所變更。模型已下載，請核對設定後重試。') : setupError(code, text));
      }
    } finally { if (operation.current === controller) operation.current = null; if (mounted.current) { setStage(''); onBusyChange?.(false); } }
  }

  const busy = !!stage;
  const labels: Record<string, string> = {
    checking: text('Checking this Mac…', '正在檢查此 Mac…'), starting: text('Starting Ollama…', '正在啟動 Ollama…'),
    server_ready: text('Engine started; preparing model…', '引擎已啟動，正在準備模型…'),
    downloading: text('Downloading model…', '正在下載模型…'), verifying: text('Verifying download…', '正在核對下載…'),
    complete: text('Download complete; preparing test…', '下載完成，準備測試…'),
    testing: text('Testing with synthetic mail…', '正在使用合成郵件測試…'), applying: text('Applying to mail…', '套用到電郵工作區…'),
  };
  const stageLabel = labels[stage] || text('Preparing…', '準備中…');
  return { probe, model, setModel, stage, stageLabel, bytes, error, setError, ready, setReady, checking, busy, refresh, prepare, cancel: () => operation.current?.abort() };
}

/**
 * `confirmPrepare`: ask before "Prepare and use for mail", which replaces the
 * mail processing mode (Settings > Advanced > Local AI). Onboarding passes none
 * and keeps the plain button.
 */
export default function LocalMailSetup({ onReady, onBusyChange, confirmPrepare }: { onReady?: () => void | Promise<void>; onBusyChange?: (busy: boolean) => void; confirmPrepare?: { message: string; label?: string } }) {
  const setup = useLocalSetup({ onReady, onBusyChange });
  return <LocalMailSetupPanel setup={setup} confirmPrepare={confirmPrepare} />;
}

/**
 * The panel for a run someone else owns. `blocked`: another run is going, so
 * this one cannot start (Settings keeps two, one per section).
 */
export function LocalMailSetupPanel({ setup, confirmPrepare, blocked = false }: { setup: ReturnType<typeof useLocalSetup>; confirmPrepare?: { message: string; label?: string }; blocked?: boolean }) {
  const { text } = useUiLocale();
  // The approved models, sizes and RAM come from ai/models.rs.
  const choices = mailSetupModels(useAiModels());
  const { probe, model, setModel, stage, stageLabel, bytes, error, setError, ready, setReady, checking, busy, refresh, prepare, cancel } = setup;
  return <section className="setup-local" aria-label={text('Local AI for mail', '電郵本機 AI')}>
    <h3>{text('Local AI, without an API key', '本機 AI，無須 API 密鑰')}</h3>
    <p>{text('Prepare Ollama, download a model, test it, then use it for mail. First installation needs your confirmation. Existing mail settings change only after a successful test.', '準備 Ollama、下載模型並完成測試後，即可套用至電郵工作區。首次安裝需要確認；只有測試成功後才會變更現有電郵設定。')}</p>
    {probe && <p>{probe.os} {probe.macosVersion} · {probe.totalRamGb.toFixed(0)} GB RAM · {probe.availableDiskGb === null ? text('Free disk unknown', '未能取得磁碟餘額') : `${probe.availableDiskGb.toFixed(1)} GB ${text('free disk', '磁碟餘額')}`}</p>}
    <Select label={text('Model to download', '要下載的模型')} showLabel value={model} disabled={busy} onChange={value => { setModel(value); setReady(false); }}
      options={[{ value: '', label: text('Choose a model', '選擇模型') }, ...choices.map(item => ({ value: item.id, label: `${item.id} · ${item.download_gb} GB · ${item.min_ram_gb} GB RAM ${text('suggested', '建議')}${probe?.recommendedTag === item.id ? text(' · recommended', ' · 推薦') : ''}`, disabled: probe ? probe.totalRamGb < item.min_ram_gb : false }))]} />
    <p className="setup-hint">{text('No cloud token fees; disk space, electricity and performance depend on your Mac. Small models may fail mail citation checks. If citations are invalid or content is too long, analysis stops and you can choose another model; mail will not switch to cloud.', '沒有雲端 token 費用；磁碟用量、電費及效能取決於 Mac 規格。小型模型可能無法提供有效的郵件引用。引用或容量檢查失敗時，分析會停止，可另選模型；系統不會自動轉用雲端。')}</p>
    <div className="setup-actions">
      <button type="button" disabled={busy || checking} onClick={() => void refresh()}>{checking ? text('Checking…', '檢查中…') : text('Check again', '重新檢查')}</button>
      {probe && !probe.ollamaInstalled && !probe.ollamaRunning && <button type="button" disabled={busy} onClick={() => void openUrl('https://ollama.com/download/mac').catch(() => setError(text('Could not open the official download page.', '未能開啟官方下載頁。')))}>{text('Install Ollama', '安裝 Ollama')}</button>}
      {confirmPrepare ? <Button kind="danger" confirm={confirmPrepare} disabled={busy || blocked || checking || !probe || !model} onClick={prepare}>{text('Prepare and use for mail', '準備並用於電郵')}</Button>
        : <button type="button" className="setup-primary" disabled={busy || blocked || checking || !probe || !model} onClick={() => void prepare()}>{text('Prepare and use for mail', '準備並用於電郵')}</button>}
      {busy && <button type="button" disabled={stage === 'applying'} onClick={cancel}>{text('Cancel', '取消')}</button>}
    </div>
    {busy && <div role="status" aria-live="polite"><p>{stageLabel}</p>{bytes && <><progress max={bytes.total} value={Math.min(bytes.done, bytes.total)} /><span> {(bytes.done / 1e9).toFixed(2)} / {(bytes.total / 1e9).toFixed(2)} GB</span></>}</div>}
    {ready && <p role="status">{text('The sample passed. Mail now uses this local model. Assistant and voice settings have their own setup.', '示範測試通過，電郵已改用此本機模型。AI 助理與語音需另行設定。')}</p>}
    {error && <p role="alert" className="setup-error">{error}</p>}
  </section>;
}
