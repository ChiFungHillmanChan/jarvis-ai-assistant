import { useEffect, useState } from 'react';
import AccountPanel from '../../components/AccountPanel';
import BackgroundSettings from '../../components/BackgroundSettings';
import { accountStatus } from '../../lib/commands';
import type { AccountStatus } from '../../lib/types';
import { accountSectionVisible, displayNameChanged } from '../../lib/account';
import { MAX_ADDRESS_CHARS, MAX_INSTRUCTIONS_CHARS, PERSONA_CHOICES, personaLabel, personaValue, type PersonaKey } from '../../lib/persona';
import { nameToShow } from '../../lib/user-name';
import { useUiLocale, type UiLocale } from '../../i18n';
import { useWorkspace } from '../../workspace/WorkspaceContext';
import SettingGroup from '../../ui/SettingGroup';
import SettingRow from '../../ui/SettingRow';
import Select from '../../ui/Select';
import Segmented from '../../ui/Segmented';
import Switch from '../../ui/Switch';
import TextField from '../../ui/TextField';
import Button from '../../ui/Button';
import { ROWS_BY_ID, rowText } from '../rows';
import { followsUi, replyLanguageForUi, replyLanguageOnLocaleChange, type ReplyLanguage } from '../language';
import { useSettingsValues, useWallpaper } from '../hooks/useSettingsValues';

type Write = [key: string, value: string];
const REPLY_LANGUAGES: ReplyLanguage[] = ['en', 'yue', 'bilingual'];
const LOCALES: { value: UiLocale; label: string }[] = [{ value: 'en', label: 'English' }, { value: 'zh-HK', label: '繁體中文（香港）' }];

/** Settings > General: account, languages, appearance, how JARVIS replies, startup. Every control saves on change. */
export default function General() {
  const { locale, setLocale, saveError: localeSaveError, retrySave, t, text } = useUiLocale();
  const { appearance, setAppearance } = useWorkspace();
  const { values, ready, error: loadError, set, reload } = useSettingsValues();
  const wallpaper = useWallpaper();
  const [accountInfo, setAccountInfo] = useState<AccountStatus | null>(null);
  // The rows whose last save failed, by row id.
  const [failed, setFailed] = useState<Record<string, boolean>>({});

  useEffect(() => { accountStatus().then(setAccountInfo).catch(() => {}); }, []);

  const row = (id: string) => ({ id, ...rowText(ROWS_BY_ID[id], locale) });
  const failure = (id: string) => failed[id] && <span className="ui-secret-error" role="alert">{text('Could not save. Try again.', '無法儲存，請再試一次。')}</span>;

  // Writes in order; any failure marks the row and stops. Resolves to whether all were saved.
  async function save(id: string, ...writes: Write[]): Promise<boolean> {
    setFailed(f => ({ ...f, [id]: false }));
    try {
      for (const [key, value] of writes) await set(key, value);
      return true;
    } catch (e) {
      console.error(e);
      setFailed(f => ({ ...f, [id]: true }));
      return false;
    }
  }
  // TextField keeps an unsaved draft open for another try only when its commit rejects.
  const commitText = (id: string, key: string) => async (v: string) => {
    if (!(await save(id, [key, v.trim()]))) throw new Error(`${key} was not saved`);
    if (key === 'assistant_address') displayNameChanged();
  };

  async function changeUiLocale(next: UiLocale) {
    if (next === locale || !(await setLocale(next))) return;
    const reply = replyLanguageOnLocaleChange(locale, next, values);
    if (reply) await save('language', ['language', reply], ['language_follows_ui', 'true']);
  }

  const reply: ReplyLanguage = REPLY_LANGUAGES.includes(values.language as ReplyLanguage) ? values.language as ReplyLanguage : 'en';
  const replyLabel = (v: ReplyLanguage) => v === 'en' ? 'English' : v === 'yue' ? text('Traditional Chinese (written)', '繁體書面語') : text('Bilingual (code-mix)', '雙語混合');
  const uiReply = replyLanguageForUi(locale);
  // Users from before the follow rule whose reply language differs from the menus: say so once, fix in one click.
  const mismatch = values.language_follows_ui == null && !followsUi(values, locale);
  const persona = (key: PersonaKey) => personaValue(key, values[key] ?? null);
  const personaOptions = (key: PersonaKey) => PERSONA_CHOICES[key].map(value => ({ value: value as string, label: personaLabel(value, text) }));
  const personaSelect = (key: PersonaKey, label: string) => (
    <Select value={persona(key)} options={personaOptions(key)} label={label} width={260} onChange={v => { void save(key, [key, v]); }} />
  );

  if (loadError) {
    return (
      <p className="ui-secret-error" role="alert">
        {text('Could not load settings.', '無法載入設定。')} <Button kind="quiet" onClick={reload}>{t('retry')}</Button>
      </p>
    );
  }

  // A change made before the saved values arrive could be overwritten by them, or judged against the wrong reply language.
  if (!ready) return <p className="ui-hint" role="status">{text('Loading settings…', '正在載入設定…')}</p>;

  return <>
    {mismatch && (
      <div className="settings-general-notice" role="status">
        <p>{text(`JARVIS replies in ${replyLabel(reply)}, not in the language of the menus.`, `JARVIS 目前以${replyLabel(reply)}回覆，與介面語言不同。`)}</p>
        <Button kind="primary" onClick={() => save('language-match', ['language', uiReply], ['language_follows_ui', 'true'])}>{text(`Reply in ${replyLabel(uiReply)}`, `改用${replyLabel(uiReply)}回覆`)}</Button>
        <Button kind="quiet" onClick={() => save('language-match', ['language_follows_ui', 'false'])}>{text('Keep it', '保持不變')}</Button>
        {failure('language-match')}
      </div>
    )}

    {accountSectionVisible(accountInfo) && (
      <SettingGroup title={row('account').title}>
        <div id="account"><AccountPanel /></div>
      </SettingGroup>
    )}

    <SettingGroup title={text('Language', '語言')}>
      <SettingRow {...row('language')} control={<Select value={locale} options={LOCALES} label={row('language').title} width={220} onChange={v => { void changeUiLocale(v); }} />}>
        {localeSaveError && <span className="ui-secret-error" role="alert">{text(localeSaveError, '未能儲存介面語言。關閉 JARVIS 前請重試。')} <Button kind="quiet" onClick={retrySave}>{t('retry')}</Button></span>}
        {failure('language')}
      </SettingRow>
      <SettingRow {...row('reply-language')} control={
        <Select value={reply} options={REPLY_LANGUAGES.map(v => ({ value: v, label: replyLabel(v) }))} label={row('reply-language').title} width={220}
          onChange={v => { void save('reply-language', ['language_follows_ui', 'false'], ['language', v]); }} />
      }>{failure('reply-language')}</SettingRow>
      {reply !== 'en' && (
        <SettingRow {...row('chinese-replies')} control={
          <Segmented value={persona('assistant_chinese')} options={personaOptions('assistant_chinese')} label={row('chinese-replies').title}
            onChange={v => { void save('chinese-replies', ['assistant_chinese', v]); }} />
        }>{failure('chinese-replies')}</SettingRow>
      )}
    </SettingGroup>

    <SettingGroup title={text('Appearance', '外觀')}>
      <SettingRow {...row('theme')} control={
        <Segmented value={appearance} label={row('theme').title} onChange={v => {
          setFailed(f => ({ ...f, theme: false }));
          setAppearance(v).catch(e => { console.error(e); setFailed(f => ({ ...f, theme: true })); });
        }}
          options={[{ value: 'light', label: text('Light', '淺色') }, { value: 'dark', label: text('Dark', '深色') }, { value: 'system', label: text('System', '系統') }]} />
      }>{failure('theme')}</SettingRow>
    </SettingGroup>

    <SettingGroup title={text('How JARVIS replies', 'JARVIS 的回覆方式')}>
      <SettingRow {...row('assistant-address')} control={
        <div style={{ width: 220 }}>
          <TextField value={values.assistant_address ?? ''} label={row('assistant-address').title} maxLength={MAX_ADDRESS_CHARS}
            placeholder={nameToShow({ ...values, assistant_address: null }) ?? text('Your name', '你的名稱')} onCommit={commitText('assistant-address', 'assistant_address')} />
        </div>
      }>{failure('assistant-address')}</SettingRow>
      <SettingRow {...row('assistant-tone')} control={personaSelect('assistant_tone', row('assistant-tone').title)}>{failure('assistant_tone')}</SettingRow>
      <SettingRow {...row('assistant-length')} control={personaSelect('assistant_length', row('assistant-length').title)}>{failure('assistant_length')}</SettingRow>
      <SettingRow {...row('assistant-search')} description={text('Thorough uses a little more of your AI allowance.', '徹底模式會略為增加 AI 用量。')}
        control={personaSelect('assistant_search', row('assistant-search').title)}>{failure('assistant_search')}</SettingRow>
      <SettingRow {...row('assistant-instructions')} control={null}>
        <InstructionsField value={values.assistant_instructions ?? ''} label={row('assistant-instructions').title} onCommit={commitText('assistant-instructions', 'assistant_instructions')} />
        {failure('assistant-instructions')}
      </SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Desktop', '桌面')}>
      <SettingRow {...row('wallpaper')} control={
        <Switch checked={wallpaper.active} disabled={wallpaper.busy} label={row('wallpaper').title}
          onChange={next => {
            setFailed(f => ({ ...f, wallpaper: false }));
            wallpaper.toggle(next).catch(e => { console.error(e); setFailed(f => ({ ...f, wallpaper: true })); });
          }} />
      }>{failure('wallpaper')}</SettingRow>
    </SettingGroup>

    <SettingGroup title={text('Startup', '啟動')}>
      {/* Open at login and keep awake are BackgroundSettings' own controls; these ids let search land here. */}
      <div id="open-at-login"><span id="keep-awake" /><BackgroundSettings /></div>
    </SettingGroup>
  </>;
}

/** The free-text reply preferences: several lines, saved when the field is left. */
function InstructionsField({ value, label, onCommit }: { value: string; label: string; onCommit(v: string): Promise<void> }) {
  const { text } = useUiLocale();
  const [draft, setDraft] = useState(value);
  useEffect(() => { setDraft(value); }, [value]);
  return <>
    <textarea className="ui-input" style={{ height: 'auto', resize: 'vertical' }} rows={4} aria-label={label} value={draft} maxLength={MAX_INSTRUCTIONS_CHARS}
      placeholder={text('For example: lead with the most urgent item.', '例如：先說最緊急的事項。')}
      onChange={e => setDraft(e.target.value)}
      onBlur={() => { if (draft.trim() !== value) onCommit(draft).catch(() => {}); }} />
    <div className="ui-hint">{`${draft.length}/${MAX_INSTRUCTIONS_CHARS} · `}{text('Tone and format only. It cannot change what JARVIS is allowed to do.', '只影響語氣及格式，不會改變 JARVIS 可以做的事。')}</div>
  </>;
}
