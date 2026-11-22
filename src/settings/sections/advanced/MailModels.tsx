import { useEffect, useState } from 'react';
import { useUiLocale } from '../../../i18n';
import type { AiModelCatalog } from '../../../lib/ai-models';
import { cloudModelFor, formatRate, mailSetupModels } from '../../../lib/ai-catalog';
import { modelChosen, type BedrockListState } from '../../../lib/bedrock-models';
import { mailSaveSettings, mailWorkspace, type MailSettings } from '../../../lib/mail';
import BedrockModelPicker from '../../../components/BedrockModelPicker';
import Button from '../../../ui/Button';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import TextField from '../../../ui/TextField';
import { mergeMailSetting, type MailModelKey as ModelKey } from '../../mail-models';
import { useRow } from './useRow';

/**
 * Advanced > Models: the model mail analysis uses for each way it can run.
 * A field saves only its own model with mail_save_settings, merged over the
 * mail settings read again just before (mergeMailSetting), so a mode that
 * choose_ai changed meanwhile is kept. `reloadKey` changes after a choice on
 * this page, so "In use for mail" follows it. Mail needs a model named
 * (mail/analysis.rs validate_model), so an empty one is refused here, and the
 * Bedrock picker works as in the mail form, with its own Save.
 */
export default function MailModels({ catalog, bedrock, reloadKey }: { catalog: AiModelCatalog; bedrock: { list: BedrockListState; reload: () => void }; reloadKey: number }) {
  const { text } = useUiLocale();
  const row = useRow();
  const [settings, setSettings] = useState<MailSettings | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [failed, setFailed] = useState<{ key: ModelKey; line: string } | null>(null);
  const [bedrockDraft, setBedrockDraft] = useState<string | null>(null);

  async function load() {
    setLoadError(false);
    try { setSettings((await mailWorkspace()).settings); } catch { setLoadError(true); }
  }
  useEffect(() => { void load(); }, [reloadKey]);

  /** Saves one model; rejects so the field keeps its draft. */
  async function save(key: ModelKey, value: string) {
    if (!settings) return;
    setFailed(null);
    if (!modelChosen(value)) {
      setFailed({ key, line: text('Enter a model. Mail cannot run without one.', '請輸入模型。郵件分析必須指定模型。') });
      throw new Error('no model');
    }
    try {
      const fresh = (await mailWorkspace()).settings;
      const next = mergeMailSetting(fresh, key, value);
      await mailSaveSettings(next);
      setSettings(next);
    } catch (e) {
      setFailed({ key, line: text(`Could not save: ${String(e)}`, `無法儲存：${String(e)}`) });
      throw e;
    }
  }
  const failure = (key: ModelKey) => failed?.key === key && <span className="ui-secret-error" role="alert">{failed.line}</span>;
  const inUse = (mode: string) => settings?.processing_mode === mode && <span className="settings-chip is-on">{text('In use for mail', '郵件正在使用')}</span>;

  if (loadError) {
    return <p className="ui-secret-error" role="alert">{text('Could not load the mail settings.', '無法載入郵件設定。')} <Button kind="quiet" onClick={load}>{text('Retry', '重試')}</Button></p>;
  }
  if (!settings) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  const example = mailSetupModels(catalog)[0]?.id ?? '';
  const openai = settings.openai_model.trim() || catalog.defaults.openai;
  const priced = cloudModelFor(catalog, openai);

  return (
    <SettingGroup title={text('Mail analysis models', '郵件分析模型')}>
      <SettingRow {...row('mail-local-model')} control={
        <div style={{ width: 240 }}>
          <TextField value={settings.local_model} label={row('mail-local-model').title}
            placeholder={example ? text(`An installed model, e.g. ${example}`, `已安裝的模型，例如 ${example}`) : text('An installed model', '已安裝的模型')}
            onCommit={v => save('local_model', v.trim())} />
        </div>
      }>{inUse('local')}{failure('local_model')}</SettingRow>

      <SettingRow {...row('mail-openai-model')} control={
        <div style={{ width: 240 }}>
          <TextField value={settings.openai_model} label={row('mail-openai-model').title} placeholder={catalog.defaults.openai} onCommit={v => save('openai_model', v.trim())} />
        </div>
      }>
        {priced && <span className="ui-hint">{text(`${openai} costs about US$${formatRate(priced.price.input_per_million)} per million input tokens. Your OpenAI account pays.`, `${openai} 每百萬輸入 token 約 US$${formatRate(priced.price.input_per_million)}，費用由你的 OpenAI 帳戶支付。`)} </span>}
        {inUse('openai')}{failure('openai_model')}
      </SettingRow>

      <SettingRow {...row('mail-bedrock-model')} control={null}>
        <BedrockModelPicker place="mail" value={bedrockDraft ?? settings.bedrock_model} stored={settings.bedrock_model} fallback={catalog.defaults.bedrock}
          listState={bedrock.list} onReload={bedrock.reload} onChange={setBedrockDraft} />
        <span className="settings-row-actions settings-row-actions-start">
          <Button kind="secondary" disabled={bedrockDraft === null || bedrockDraft === settings.bedrock_model}
            onClick={async () => { await save('bedrock_model', bedrockDraft ?? ''); setBedrockDraft(null); }} onError={() => {}}>{text('Save model', '儲存模型')}</Button>
          {inUse('bedrock')}
        </span>
        {failure('bedrock_model')}
      </SettingRow>
    </SettingGroup>
  );
}
