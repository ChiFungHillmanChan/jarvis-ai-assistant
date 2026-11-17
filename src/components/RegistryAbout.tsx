import { useUiLocale } from '../i18n';
import type { AiModelCatalog } from '../lib/ai-models';
import { registrySourceLabel } from '../lib/ai-catalog';
import { formatPlanDate } from '../lib/paywall';

/**
 * Settings > About: the app version and the model list (model registry) in
 * use, with where it came from. The list is read through ai_models, so this
 * follows a newer registry as soon as it is put in use.
 */
export default function RegistryAbout({ catalog }: { catalog: AiModelCatalog | null }) {
  const { locale, text } = useUiLocale();
  if (!catalog) return <div className="settings-panel"><p role="status" className="settings-help">{text('Loading…', '正在載入…')}</p></div>;
  const { registry } = catalog;
  return (
    <div className="settings-panel">
      <h2 className="settings-heading">{text('About JARVIS', '關於 JARVIS')}</h2>
      <dl className="settings-about">
        <dt>{text('Version', '版本')}</dt><dd>{catalog.app_version}</dd>
        <dt>{text('Model list version', '模型清單版本')}</dt><dd>{registry.version}</dd>
        <dt>{text('Model list source', '模型清單來源')}</dt><dd>{registrySourceLabel(registry.source, text)}</dd>
        <dt>{text('Model list issued', '模型清單發佈日期')}</dt><dd>{formatPlanDate(registry.issued_at, locale)}</dd>
      </dl>
      <p className="settings-help" style={{ maxWidth: '72ch' }}>{catalog.registry_updates
        ? text('The model list sets the models, prices and limits JARVIS uses. JARVIS checks for a newer list when it opens, every 6 hours and when this Mac wakes, and uses one only if its signature is valid. Your API keys are never sent with these checks.', '模型清單決定 JARVIS 使用的模型、價目及限制。JARVIS 會在開啟時、每 6 小時及此 Mac 喚醒後檢查有否較新的清單，並只會使用簽署有效的清單。檢查時不會傳送你的 API 密鑰。')
        : text('The model list sets the models, prices and limits JARVIS uses. This version does not check for a newer list; the list changes when JARVIS is updated.', '模型清單決定 JARVIS 使用的模型、價目及限制。此版本不會檢查有否較新的清單；清單會隨 JARVIS 更新而更新。')}</p>
    </div>
  );
}
