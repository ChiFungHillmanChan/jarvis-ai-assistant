// The Bedrock model picker (Settings > Models > Provider priority): the
// options it offers, the line saying which list they come from, and the
// picker's own state. The list comes from list_bedrock_models
// (src-tauri/src/aws/model_list.rs): the inference profiles and text models
// AWS lists for the saved region, or a built-in list and the reason.

export type Text = (en: string, zh: string) => string;

export type BedrockModelChoice = { id: string; name: string; kind: 'profile' | 'model' };

export type BedrockListReason =
  | 'no_credentials'
  | 'unreadable_credentials'
  | 'credentials_refused'
  | 'not_allowed'
  | 'offline'
  | 'timed_out'
  | 'failed';

export type BedrockModelList = {
  source: 'aws' | 'fallback';
  region: string | null;
  reason: BedrockListReason | null;
  /** For not_allowed: the IAM actions AWS refused, as AWS names them. */
  missing_permissions: string[];
  /** What the empty choice runs. */
  default_model: { id: string; name: string | null };
  /** Inference profiles first, then plain model ids. */
  models: BedrockModelChoice[];
};

/** Where the picker's list stands while the Models section is open. */
export type BedrockListState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'ready'; list: BedrockModelList }
  | { status: 'error' };

/**
 * The value of "Other (enter an ID)". A saved model id never holds a space
 * (bedrock_model_to_save refuses whitespace), so no id can be this value.
 */
export const OTHER_MODEL = ' other';

export type PickerGroup = 'default' | 'saved' | 'profiles' | 'models' | 'built-in' | 'other';
export type PickerOption = { value: string; label: string; group: PickerGroup };

function named(name: string | null | undefined, id: string, text: Text): string {
  return name && name !== id ? text(`${name} (${id})`, `${name}（${id}）`) : id;
}

/**
 * The picker's options, in order: the default (the empty choice), an id
 * saved earlier that the list does not hold, the listed models (AWS's
 * profiles, then its models; or the built-in list), and "Other".
 * `fallbackDefault` names the default until the list has arrived.
 */
export function pickerOptions(list: BedrockModelList | null, saved: string, fallbackDefault: string, text: Text): PickerOption[] {
  const defaultId = list?.default_model.id || fallbackDefault;
  const defaultName = list?.default_model.name ?? null;
  const options: PickerOption[] = [{
    value: '',
    group: 'default',
    label: defaultId
      ? text(`Default: ${named(defaultName, defaultId, text)}`, `預設：${named(defaultName, defaultId, text)}`)
      : text('Default model', '預設模型'),
  }];
  const listed = list?.models ?? [];
  if (saved && !listed.some((model) => model.id === saved)) {
    options.push({ value: saved, group: 'saved', label: text(`Saved: ${saved}`, `已儲存：${saved}`) });
  }
  for (const model of listed) {
    const group: PickerGroup = list?.source === 'aws' ? (model.kind === 'profile' ? 'profiles' : 'models') : 'built-in';
    options.push({ value: model.id, group, label: named(model.name, model.id, text) });
  }
  options.push({ value: OTHER_MODEL, group: 'other', label: text('Other (enter an ID)', '其他（輸入 ID）') });
  return options;
}

/** The heading of an option group in the select, or null for none. */
export function groupLabel(group: PickerGroup, text: Text): string | null {
  switch (group) {
    case 'profiles': return text('Inference profiles from AWS', 'AWS 推論設定檔');
    case 'models': return text('Models from AWS', 'AWS 模型');
    case 'built-in': return text('Built-in list', '內置清單');
    default: return null;
  }
}

/** The options split into runs of one group, for <optgroup>. */
export function groupedOptions(options: PickerOption[]): { group: PickerGroup; options: PickerOption[] }[] {
  const runs: { group: PickerGroup; options: PickerOption[] }[] = [];
  for (const option of options) {
    const last = runs[runs.length - 1];
    if (last && last.group === option.group) last.options.push(option);
    else runs.push({ group: option.group, options: [option] });
  }
  return runs;
}

function allowed(permissions: string[], text: Text): string {
  return permissions.join(text(' and ', ' 及 '));
}

/**
 * The line under the picker: which list it shows, and for the built-in list
 * why. AWS's list is what AWS lists for the region; nothing in it says the
 * account has access to a model, so this line does not say so either.
 */
export function listStatus(state: BedrockListState, text: Text): string {
  if (state.status === 'idle') return text('The model list has not been loaded yet.', '尚未載入模型清單。');
  if (state.status === 'loading') return text('Loading the model list…', '正在載入模型清單……');
  if (state.status === 'error') return text('JARVIS could not load the model list. Load it again to retry.', 'JARVIS 未能載入模型清單，可重新載入再試。');
  const { list } = state;
  if (list.source === 'aws') {
    const region = list.region ?? '';
    return text(
      `List from your AWS account for ${region}. AWS can still refuse a listed model if your account has no access to it.`,
      `清單來自你的 AWS 帳戶（地區：${region}）。如帳戶未獲某個模型的存取權限，即使該模型在清單中，AWS 仍可能拒絕。`,
    );
  }
  switch (list.reason) {
    case 'no_credentials':
      return text('Built-in list: no AWS credentials are saved. Save them under AWS Bedrock below to load the list from your AWS account.', '內置清單：尚未儲存 AWS 憑證。請在下方「AWS Bedrock」儲存憑證，即可從你的 AWS 帳戶載入清單。');
    case 'unreadable_credentials':
      return text('Built-in list: JARVIS could not read your saved AWS credentials. Save them again under AWS Bedrock below.', '內置清單：JARVIS 未能讀取已儲存的 AWS 憑證，請在下方「AWS Bedrock」重新儲存。');
    case 'credentials_refused':
      return text('Built-in list: AWS did not accept the saved AWS credentials. Check them under AWS Bedrock below.', '內置清單：AWS 不接受已儲存的 AWS 憑證，請在下方「AWS Bedrock」檢查。');
    case 'not_allowed':
      return text(
        `Built-in list: your AWS key is not allowed to list models. To load the list from your account, allow ${allowed(list.missing_permissions, text)} in IAM.`,
        `內置清單：你的 AWS 密鑰無權列出模型。如要從帳戶載入清單，請在 IAM 允許 ${allowed(list.missing_permissions, text)}。`,
      );
    case 'offline':
      return text('Built-in list: JARVIS could not connect to AWS. Check the internet connection, then load the list again.', '內置清單：JARVIS 未能連接 AWS。請檢查網絡連線，然後重新載入清單。');
    case 'timed_out':
      return text('Built-in list: AWS did not send the list in time. Load it again to retry.', '內置清單：AWS 未能及時傳回清單，可重新載入再試。');
    default:
      return text('Built-in list: AWS could not send the list. Load it again to retry.', '內置清單：AWS 未能傳回清單，可重新載入再試。');
  }
}

/** The picker's own state. The saved id lives in the chain row, not here. */
export type PickerState = {
  /** "Other (enter an ID)" is chosen: the ID field shows. */
  other: boolean;
  /** The ID field's text. */
  draft: string;
  /** The value being saved, shown while the save runs. */
  pending: string | null;
  /** The last refusal, shown until the user changes the choice or the text. */
  error: string | null;
};

export type PickerAction =
  | { type: 'choose'; value: string; saved: string }
  | { type: 'type'; draft: string }
  | { type: 'saving'; value: string }
  | { type: 'saved'; error: string | null }
  | { type: 'synced'; saved: string };

export function initialPicker(saved: string): PickerState {
  return { other: false, draft: saved, pending: null, error: null };
}

export function pickerReducer(state: PickerState, action: PickerAction): PickerState {
  switch (action.type) {
    case 'choose':
      return action.value === OTHER_MODEL
        ? { ...state, other: true, draft: action.saved, error: null }
        : { ...state, other: false, error: null };
    case 'type':
      return { ...state, draft: action.draft, error: null };
    case 'saving':
      return { ...state, pending: action.value, error: null };
    case 'saved':
      return { ...state, pending: null, error: action.error, other: action.error ? state.other : false };
    case 'synced':
      return state.other ? state : { ...state, draft: action.saved };
  }
}

/** What the select shows: "Other" while it is chosen, else the value being saved, else the saved id. */
export function selectValue(state: PickerState, saved: string): string {
  if (state.other) return OTHER_MODEL;
  return state.pending ?? saved;
}
