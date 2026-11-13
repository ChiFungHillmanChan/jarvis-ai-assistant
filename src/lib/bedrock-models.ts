// The Bedrock model picker: the options it offers, the line saying which
// list they come from, and the picker's own state. One picker serves three
// places: Settings > Models > Provider priority (a Bedrock row), the Mail
// page's Analysis settings (group analysis with "Your AWS Bedrock account"),
// and the setup guide's "Advanced: use a company AWS Bedrock account for
// mail". The list comes from list_bedrock_models
// (src-tauri/src/aws/model_list.rs): the inference profiles and text models
// AWS lists for the saved region, or a built-in list and the reason.

export type Text = (en: string, zh: string) => string;

/**
 * Where the picker is. In Settings a choice is saved at once and the empty
 * choice runs the default model. In the Mail dialog and the setup guide a
 * choice goes into the form, whose own button saves it, and a model is
 * required.
 */
export type PickerPlace = 'settings' | 'mail' | 'setup';

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

export type PickerGroup = 'choose' | 'default' | 'saved' | 'profiles' | 'models' | 'built-in' | 'other';
export type PickerOption = { value: string; label: string; group: PickerGroup; disabled?: true };

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
  const listed = listedOptions(list, text);
  if (saved && !listed.some((option) => option.value === saved)) {
    options.push({ value: saved, group: 'saved', label: text(`Saved: ${saved}`, `已儲存：${saved}`) });
  }
  options.push(...listed, otherOption(text));
  return options;
}

/** The listed models: AWS's profiles, then its models; or the built-in list. */
function listedOptions(list: BedrockModelList | null, text: Text): PickerOption[] {
  return (list?.models ?? []).map((model): PickerOption => ({
    value: model.id,
    group: list?.source === 'aws' ? (model.kind === 'profile' ? 'profiles' : 'models') : 'built-in',
    label: named(model.name, model.id, text),
  }));
}

function otherOption(text: Text): PickerOption {
  return { value: OTHER_MODEL, group: 'other', label: text('Other (enter an ID)', '其他（輸入 ID）') };
}

/**
 * The picker's options in the Mail dialog and the setup guide, where a model
 * is required, so there is no empty "Default" choice. In order: "Choose a
 * model" while the form holds no id; the app's default model by name, when
 * the list does not hold it; the id saved in mail settings, when the list
 * does not hold it; the form's id, when it is none of these (picked from a
 * list loaded again since); the listed models; "Other". `chosen` is the
 * form's id, or null while "Other" is chosen and its field holds the id.
 */
export function formPickerOptions(list: BedrockModelList | null, chosen: string | null, stored: string, fallbackDefault: string, text: Text): PickerOption[] {
  const listed = listedOptions(list, text);
  const offered = new Set(listed.map((option) => option.value));
  const options: PickerOption[] = [];
  if (chosen === '') options.push({ value: '', group: 'choose', label: text('Choose a model', '選擇模型'), disabled: true });
  const defaultId = list?.default_model.id || fallbackDefault;
  if (defaultId && !offered.has(defaultId)) {
    options.push({ value: defaultId, group: 'default', label: named(list?.default_model.name, defaultId, text) });
    offered.add(defaultId);
  }
  if (stored && !offered.has(stored)) {
    options.push({ value: stored, group: 'saved', label: text(`Saved: ${stored}`, `已儲存：${stored}`) });
    offered.add(stored);
  }
  if (chosen && !offered.has(chosen)) {
    options.push({ value: chosen, group: 'saved', label: text(`Chosen: ${chosen}`, `已選擇：${chosen}`) });
  }
  options.push(...listed, otherOption(text));
  return options;
}

/**
 * Whether a form holds a model id. Mail's Bedrock model cannot be empty
 * (group analysis and the setup test refuse it), so the Mail dialog and the
 * setup guide do not save Bedrock without one.
 */
export function modelChosen(value: string): boolean {
  return value.trim() !== '';
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
export function listStatus(state: BedrockListState, text: Text, place: PickerPlace = 'settings'): string {
  if (state.status === 'idle') return text('The model list has not been loaded yet.', '尚未載入模型清單。');
  if (state.status === 'loading') return text('Loading the model list…', '正在載入模型清單……');
  if (state.status === 'error') return text('JARVIS could not load the model list. Load it again to retry.', 'JARVIS 未能載入模型清單，可重新載入再試。');
  const { list } = state;
  if (list.source === 'aws') {
    const region = list.region ?? '';
    // The setup guide also takes new keys; the list is from the saved ones.
    return place === 'setup'
      ? text(
        `List from the AWS account of your saved credentials, for ${region}. AWS can still refuse a listed model if your account has no access to it.`,
        `清單來自已儲存憑證的 AWS 帳戶（地區：${region}）。如帳戶未獲某個模型的存取權限，即使該模型在清單中，AWS 仍可能拒絕。`,
      )
      : text(
        `List from your AWS account for ${region}. AWS can still refuse a listed model if your account has no access to it.`,
        `清單來自你的 AWS 帳戶（地區：${region}）。如帳戶未獲某個模型的存取權限，即使該模型在清單中，AWS 仍可能拒絕。`,
      );
  }
  return `${fallbackReason(list, text, place)}${text(` ${BUILT_IN_REGION_NOTE[0]}`, BUILT_IN_REGION_NOTE[1])}`;
}

/**
 * The last sentence of every built-in list line: its ids are the ones the
 * app ships, and AWS's own pages do not show each of them for every region
 * (Nova Micro's APAC profile, for one).
 */
export const BUILT_IN_REGION_NOTE = ['AWS may not offer every model in this list in your region.', 'AWS 未必在你的地區提供此清單內的每個模型。'] as const;

/**
 * Where the saved credentials are fixed, for the three reasons about them.
 * Settings has them in its AWS Bedrock panel below the picker; the Mail
 * dialog does not, they are in Settings → Models; the setup guide takes new
 * keys above the picker and saves them with its test button.
 */
function credentialsReason(reason: 'no_credentials' | 'unreadable_credentials' | 'credentials_refused', place: Exclude<PickerPlace, 'settings'>, text: Text): string {
  if (place === 'mail') {
    switch (reason) {
      case 'no_credentials':
        return text('Built-in list: no AWS credentials are saved. Save them in Settings → Models, under AWS Bedrock, to load the list from your AWS account.', '內置清單：尚未儲存 AWS 憑證。請在設定的「模型」頁面「AWS Bedrock」部分儲存憑證，即可從你的 AWS 帳戶載入清單。');
      case 'unreadable_credentials':
        return text('Built-in list: JARVIS could not read your saved AWS credentials. Save them again in Settings → Models, under AWS Bedrock.', '內置清單：JARVIS 未能讀取已儲存的 AWS 憑證，請在設定的「模型」頁面「AWS Bedrock」部分重新儲存。');
      case 'credentials_refused':
        return text('Built-in list: AWS did not accept the saved AWS credentials. Check them in Settings → Models, under AWS Bedrock.', '內置清單：AWS 不接受已儲存的 AWS 憑證，請在設定的「模型」頁面「AWS Bedrock」部分檢查。');
    }
  }
  switch (reason) {
    case 'no_credentials':
      return text('Built-in list: no AWS credentials are saved. The keys entered above are saved when you press “Test and use Bedrock for mail”, and JARVIS then asks your AWS account for the list.', '內置清單：尚未儲存 AWS 憑證。上方輸入的密鑰會在按「測試並以 Bedrock 處理電郵」時儲存，之後 JARVIS 會向你的 AWS 帳戶查詢清單。');
    case 'unreadable_credentials':
      return text('Built-in list: JARVIS could not read your saved AWS credentials. Enter them again above; they are saved when you press “Test and use Bedrock for mail”.', '內置清單：JARVIS 未能讀取已儲存的 AWS 憑證。請在上方重新輸入，按「測試並以 Bedrock 處理電郵」時便會儲存。');
    case 'credentials_refused':
      return text('Built-in list: AWS did not accept the saved AWS credentials. Enter new keys above to replace them; they are saved when you press “Test and use Bedrock for mail”.', '內置清單：AWS 不接受已儲存的 AWS 憑證。可在上方輸入新的密鑰以取代，按「測試並以 Bedrock 處理電郵」時便會儲存。');
  }
}

function fallbackReason(list: BedrockModelList, text: Text, place: PickerPlace): string {
  if (place !== 'settings' && (list.reason === 'no_credentials' || list.reason === 'unreadable_credentials' || list.reason === 'credentials_refused')) {
    return credentialsReason(list.reason, place, text);
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

/**
 * The help under the ID field of "Other (enter an ID)", true for where it is
 * shown. Chat points a us., eu. or apac. profile at the saved region when it
 * sends (ai/mod.rs apply_inference_profile); mail sends the id as saved
 * (mail/analysis.rs send_to_bedrock). A mail form cannot be left empty.
 */
export function otherIdHelp(place: PickerPlace, fallback: string, text: Text): string {
  switch (place) {
    case 'settings':
      return text(
        `A model ID or an inference profile ID from the Bedrock console. A us., eu. or apac. profile is sent for the region of your saved AWS credentials; other IDs are sent as typed. Leave it empty for ${fallback || "the default model"}. Usage is recorded against the ID that is sent.`,
        `可輸入 Bedrock 主控台的模型 ID 或推論設定檔 ID。us.、eu. 或 apac. 開頭的設定檔會按已儲存 AWS 憑證的地區發送，其他 ID 按輸入內容發送。留空即使用 ${fallback || "預設模型"}。用量按實際發送的 ID 記錄。`,
      );
    case 'mail':
      return text(
        'A model ID or an inference profile ID from the Bedrock console. Group analysis sends it as typed, and usage is recorded against it.',
        '可輸入 Bedrock 主控台的模型 ID 或推論設定檔 ID。群組分析會按輸入內容發送此 ID，用量亦按此 ID 記錄。',
      );
    case 'setup':
      return text(
        'A model ID or an inference profile ID from the Bedrock console. The test and later mail analysis send it as typed, and usage is recorded against it.',
        '可輸入 Bedrock 主控台的模型 ID 或推論設定檔 ID。測試及之後的電郵分析會按輸入內容發送此 ID，用量亦按此 ID 記錄。',
      );
  }
}
