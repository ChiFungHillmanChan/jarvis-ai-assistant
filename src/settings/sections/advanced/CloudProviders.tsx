import { useEffect, useState } from 'react';
import { useUiLocale } from '../../../i18n';
import { clearAwsCredentials, getApiKeyStatus, getAwsCredentialsStatus, removeApiKey, setApiKey, setAwsCredentials } from '../../../lib/commands';
import { mailWorkspace } from '../../../lib/mail';
import { AWS_REGIONS } from '../../../lib/aws-regions';
import type { AwsCredsStatus } from '../../../lib/types';
import Button from '../../../ui/Button';
import SecretField from '../../../ui/SecretField';
import Select from '../../../ui/Select';
import SettingGroup from '../../../ui/SettingGroup';
import SettingRow from '../../../ui/SettingRow';
import { useRow } from './useRow';

type Key = 'gemini' | 'claude' | 'openai' | 'typesafe';
type Copy = { en: string; zh: string };

/** One field per key, each entered only here. `stops` is what the key's Remove ends. */
const KEYS: { id: Key; row: string; name: string; stops: Copy }[] = [
  { id: 'gemini', row: 'key-gemini', name: 'Google Gemini', stops: { en: 'Chat, voice and mail that use Gemini stop until you add a key again.', zh: '使用 Gemini 的對話、語音及郵件功能會停止，直至你再次加入密鑰。' } },
  { id: 'claude', row: 'key-claude', name: 'Anthropic Claude', stops: { en: 'Requests that use Claude stop until you add a key again.', zh: '使用 Claude 的請求會停止，直至你再次加入密鑰。' } },
  { id: 'openai', row: 'key-openai', name: 'OpenAI', stops: { en: 'Requests and mail analysis that use OpenAI stop until you add a key again.', zh: '使用 OpenAI 的請求及郵件分析會停止，直至你再次加入密鑰。' } },
  { id: 'typesafe', row: 'key-typesafe', name: 'TypeSafe', stops: { en: 'Mail sorting goes back to using only the AI you chose, which costs more.', zh: '郵件整理會改為只使用你選擇的 AI，成本會較高。' } },
];

/**
 * Settings > Advanced > Cloud providers: the provider API keys (Keychain,
 * through set_api_key) and the AWS credentials Bedrock uses. A saved key is
 * never shown; each Remove asks first and says what stops.
 */
export default function CloudProviders() {
  const { text } = useUiLocale();
  const row = useRow();
  const [keys, setKeys] = useState<Record<string, boolean> | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [aws, setAws] = useState<AwsCredsStatus>({ configured: false, region: null });
  const [accessKey, setAccessKey] = useState('');
  const [secret, setSecret] = useState('');
  const [region, setRegion] = useState('ap-southeast-1');
  const [awsError, setAwsError] = useState('');
  const [removeFailed, setRemoveFailed] = useState(false);
  const [refreshNote, setRefreshNote] = useState('');

  async function readKeys() {
    // TypeSafe's saved state comes from the mail workspace, which never returns the key.
    const [status, mail] = await Promise.all([getApiKeyStatus(), mailWorkspace()]);
    setKeys({ ...status, typesafe: mail.triage.typesafe_key });
  }
  async function loadKeys() {
    setLoadError(false);
    try { await readKeys(); } catch { setLoadError(true); }
  }
  useEffect(() => {
    void loadKeys();
    getAwsCredentialsStatus().then(status => { setAws(status); if (status.region) setRegion(status.region); }).catch(console.error);
  }, []);

  /** After a save or remove that worked, a failed status read is not reported as that action failing. */
  async function refreshAfter(id: Key, saved: boolean, note: string) {
    try {
      await readKeys();
    } catch (e) {
      console.error(e);
      setKeys(k => (k ? { ...k, [id]: saved } : k));
      setRefreshNote(note);
    }
  }
  async function saveKey(id: Key, value: string) {
    setRefreshNote('');
    await setApiKey(id, value);
    await refreshAfter(id, true, text('Saved. Could not refresh status.', '已儲存，但未能更新狀態。'));
  }
  async function dropKey(id: Key) {
    setRefreshNote('');
    await removeApiKey(id);
    await refreshAfter(id, false, text('Removed. Could not refresh status.', '已移除，但未能更新狀態。'));
  }
  async function saveAws() {
    setAwsError('');
    try {
      await setAwsCredentials(accessKey.trim(), secret.trim(), region);
      setAccessKey(''); setSecret('');
      setAws(await getAwsCredentialsStatus());
    } catch (e) {
      setAwsError(text(`Could not save the AWS keys: ${String(e)}`, `無法儲存 AWS 密鑰：${String(e)}`));
    }
  }

  if (loadError) {
    return <p className="ui-secret-error" role="alert">{text('Could not read which keys are saved.', '無法讀取已儲存的密鑰。')} <Button kind="quiet" onClick={loadKeys}>{text('Retry', '重試')}</Button></p>;
  }
  if (!keys) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  const regions = AWS_REGIONS.some(r => r.id === region) ? AWS_REGIONS : [...AWS_REGIONS, { id: region, label: region }];

  return <>
    <SettingGroup title={text('Provider API keys', '供應商 API 密鑰')}>
      <p className="ui-hint">{text('Keys are kept in the macOS Keychain and never shown again. To change one, choose Replace and paste the new key.', '密鑰保存在 macOS 鑰匙圈中，儲存後不會再顯示。如需更換，請按「更換」並貼上新密鑰。')}</p>
      {KEYS.map(k => (
        <SettingRow key={k.id} {...row(k.row)} control={
          <div style={{ width: 340 }}>
            <SecretField saved={!!keys[k.id]} label={text(`${k.name} API key`, `${k.name} API 密鑰`)} placeholder={text(`Paste your ${k.name} key`, `貼上你的 ${k.name} 密鑰`)}
              onSave={v => saveKey(k.id, v)} onRemove={() => dropKey(k.id)}
              removeConfirm={text(`Remove the saved ${k.name} key from the Keychain? ${k.stops.en} This cannot be undone.`, `要從鑰匙圈移除已儲存的 ${k.name} 密鑰嗎？${k.stops.zh}此操作無法復原。`)} />
          </div>
        } />
      ))}
      {refreshNote && <p className="ui-hint" role="status">{refreshNote}</p>}
    </SettingGroup>

    <SettingGroup title="AWS Bedrock">
      <p className="ui-hint">
        {text('Use a model and region your AWS account can reach. Saving the keys does not test them.', '請使用你的 AWS 帳戶可用的模型及地區。儲存密鑰不代表已測試連接。')}
        {aws.configured && <> {text(`Saved for ${aws.region ?? ''}.`, `已為 ${aws.region ?? ''} 儲存。`)}</>}
      </p>
      <SettingRow {...row('aws-access-key')} control={
        <div style={{ width: 340 }}>
          <input className="ui-input" aria-label={row('aws-access-key').title} value={accessKey} spellCheck={false} autoComplete="off"
            placeholder={aws.configured ? text('Saved. Enter new keys to replace them', '已儲存。輸入新密鑰即可更換') : 'AKIA…'} onChange={e => setAccessKey(e.target.value)} />
        </div>
      }>{aws.configured && <span className="ui-secret-saved">{text('Saved in Keychain', '已儲存在鑰匙圈')}</span>}</SettingRow>

      <SettingRow {...row('aws-secret-key')} control={
        <div style={{ width: 340 }}>
          <input className="ui-input" type="password" aria-label={row('aws-secret-key').title} value={secret} spellCheck={false} autoComplete="off"
            placeholder={text('Secret access key', '秘密存取密鑰')} onChange={e => setSecret(e.target.value)} />
        </div>
      }>
        {aws.configured && (
          <Button kind="danger"
            confirm={{ message: text('Remove the saved AWS keys from the Keychain? Bedrock in the provider order and Bedrock mail analysis stop working until you enter them again. This cannot be undone.', '要從鑰匙圈移除已儲存的 AWS 密鑰嗎？供應商次序中的 Bedrock 及 Bedrock 郵件分析會停止運作，直至你重新輸入密鑰。此操作無法復原。'), label: text('Remove', '移除') }}
            onError={e => { console.error(e); setRemoveFailed(true); }}
            onClick={async () => { setRemoveFailed(false); await clearAwsCredentials(); setAws({ configured: false, region: null }); }}>
            {text('Remove key', '移除密鑰')}
          </Button>
        )}
        {removeFailed && <span className="ui-secret-error" role="alert">{text('Could not remove the AWS key. Try again.', '無法移除 AWS 密鑰，請再試一次。')}</span>}
      </SettingRow>

      <SettingRow {...row('aws-region')} control={<Select value={region} options={regions.map(r => ({ value: r.id, label: r.label }))} label={row('aws-region').title} width={260} onChange={setRegion} />}>
        <span className="settings-row-actions settings-row-actions-start">
          <Button kind="primary" disabled={!accessKey.trim() || !secret.trim()} onClick={saveAws}>{text('Save keys and region', '儲存密鑰及地區')}</Button>
        </span>
        {aws.configured && aws.region !== region && <span className="ui-hint">{text('Enter the keys again to use this region.', '請重新輸入密鑰以使用此地區。')}</span>}
        {awsError && <span className="ui-secret-error" role="alert">{awsError}</span>}
      </SettingRow>
    </SettingGroup>
  </>;
}
