import { useEffect, useState } from 'react';
import { modelChosen } from '../../lib/bedrock-models.ts';
import { mailSaveSettings, mailSetAutoTriage, mailWorkspace, type MailSettings, type MailTriageStatus } from '../../lib/mail.ts';

type Text = (en: string, zh: string) => string;

export interface CanSaveInput {
  processingMode: string;
  /** The group-analysis tick. */
  analysisConsent: boolean;
  /** Sorting is on, or the user asked for it and has not ticked yet. */
  sorting: boolean;
  /** The "email text is sent to the AI" tick. */
  sortingConsent: boolean;
  /** The saved Bedrock model; it is chosen in Advanced > Models. */
  bedrockModel: string;
}

/**
 * The Save rule of the old Mail page dialog (EmailPage.tsx, before Task 13):
 * the analysis tick was required for every mode except Ollama on this Mac,
 * the sorting tick only while sorting was ticked, and Bedrock needed a model.
 */
export function canSave(i: CanSaveInput): boolean {
  return (i.processingMode === 'local' || i.analysisConsent)
    && (!i.sorting || i.sortingConsent)
    && (i.processingMode !== 'bedrock' || modelChosen(i.bedrockModel));
}

/** Why Save is off, in the order canSave checks; null when it is on. */
export function saveBlockedReason(i: CanSaveInput, text: Text): string | null {
  if (i.processingMode !== 'local' && !i.analysisConsent) return text('Tick "Group analysis consent" first.', '請先勾選「群組分析同意」。');
  if (i.sorting && !i.sortingConsent) return text('Tick "Email text is sent to the AI", or turn sorting off.', '請勾選「郵件文字會傳送給 AI」，或關閉 AI 整理。');
  if (i.processingMode === 'bedrock' && !modelChosen(i.bedrockModel)) return text('Choose a Bedrock model in Advanced → Models first.', '請先在進階 → 模型中選擇 Bedrock 模型。');
  return null;
}

/**
 * What Save writes: the settings just read again, with only this form's field
 * replaced. The consent ticks are not stored (MailSettings has no consent
 * field), and the models belong to Advanced > Models, so a model chosen there
 * meanwhile is kept. mail_save_settings writes every field.
 */
export function mergeMailForm(fresh: MailSettings, form: Pick<MailSettings, 'processing_mode'>): MailSettings {
  return { ...fresh, processing_mode: form.processing_mode };
}

/** Settings > Mail and follow-ups: the sorting switch saves on change; the analysis form has its own Save. */
export function useMailSettings(text: Text) {
  const [settings, setSettings] = useState<MailSettings | null>(null);
  const [triage, setTriage] = useState<MailTriageStatus | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [mode, setModeState] = useState('');
  const [analysisConsent, setAnalysisConsent] = useState(false);
  const [sorting, setSorting] = useState(false);
  // The switch was turned on before the consent tick: it turns on once ticked.
  const [pending, setPending] = useState(false);
  const [sortingConsent, setSortingConsent] = useState(false);
  const [sortBusy, setSortBusy] = useState(false);
  const [sortError, setSortError] = useState('');
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState('');
  const [saved, setSaved] = useState(false);

  async function load() {
    setLoadError(false);
    try {
      const data = await mailWorkspace();
      setSettings(data.settings);
      setTriage(data.triage);
      setModeState(data.settings.processing_mode);
      // As in the old dialog: a saved cloud mode was agreed to when it was saved.
      setAnalysisConsent(data.settings.processing_mode !== 'local');
      setSorting(data.triage.enabled);
      setSortingConsent(data.triage.enabled);
      setPending(false);
    } catch (e) {
      console.error(e);
      setLoadError(true);
    }
  }
  useEffect(() => { void load(); }, []);

  async function setAutoSort(next: boolean) {
    const previous = sorting;
    setSortError('');
    setPending(false);
    setSorting(next);
    setSortBusy(true);
    try {
      await mailSetAutoTriage(next);
      if (!next) setSortingConsent(false);
    } catch (e) {
      console.error(e);
      setSorting(previous);
      setSortError(text('Could not change AI sorting. Try again.', '無法更改 AI 整理設定，請再試一次。'));
    } finally {
      setSortBusy(false);
    }
  }

  // The old order of checks: sorting is asked for, then the consent tick is
  // needed before it is saved on. Turning it off clears the tick, as before.
  function toggleSorting(next: boolean) {
    if (next) {
      if (sortingConsent) void setAutoSort(true);
      else setPending(true);
    } else if (pending) {
      setPending(false);
    } else {
      void setAutoSort(false);
    }
  }
  function tickSortingConsent(next: boolean) {
    setSortingConsent(next);
    if (next && pending) void setAutoSort(true);
  }

  function setMode(next: string) {
    setModeState(next);
    setAnalysisConsent(false);
    setSaved(false);
  }

  const saveInput: CanSaveInput = { processingMode: mode, analysisConsent, sorting: sorting || pending, sortingConsent, bedrockModel: settings?.bedrock_model ?? '' };

  async function save() {
    setSaveError('');
    setSaved(false);
    setSaving(true);
    try {
      const fresh = (await mailWorkspace()).settings;
      const next = mergeMailForm(fresh, { processing_mode: mode });
      // A Bedrock model removed in Advanced meanwhile turns Save off again.
      if (!canSave({ ...saveInput, bedrockModel: next.bedrock_model })) { setSettings(fresh); return; }
      await mailSaveSettings(next);
      setSettings(next);
      setSaved(true);
    } catch (e) {
      console.error(e);
      setSaveError(text(`Could not save: ${String(e)}`, `無法儲存：${String(e)}`));
    } finally {
      setSaving(false);
    }
  }

  return {
    ready: settings !== null && triage !== null, settings, triage, loadError, reload: load,
    sorting, pending, sortingConsent, sortBusy, sortError, toggleSorting, tickSortingConsent,
    mode, setMode, analysisConsent, setAnalysisConsent,
    savable: canSave(saveInput), blockedReason: saveBlockedReason(saveInput, text), save, saving, saveError, saved,
  };
}
