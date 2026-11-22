// The key steps for each cloud AI service, as data. The component renders
// them; it names no step and no link. Step 0 is always "open the key page":
// `guideFor` replaces its link with the registry's `key_page` (setup[]) when
// there is one, so the links below are only the fallback.
import type { CompatibleService, SetupChoice } from '../../lib/ai-models';

export interface ProviderGuide {
  id: 'gemini' | 'openai' | 'claude' | 'kimi' | 'qwen';
  steps: { en: string; zh: string; link?: string }[];
}

/** The save button's label in Your AI. The guides' last step names it, so both come from here. */
export const TEST_AND_SAVE = { en: 'Test and save', zh: '測試並儲存' };
const pasteStep = { en: `Paste the key here, then press "${TEST_AND_SAVE.en}".`, zh: `把密鑰貼到這裡，然後按「${TEST_AND_SAVE.zh}」。` };

export const PROVIDER_GUIDES: ProviderGuide[] = [
  {
    id: 'gemini',
    steps: [
      { en: 'Open the Google AI Studio key page and sign in.', zh: '開啟 Google AI Studio 的密鑰頁面並登入。', link: 'https://aistudio.google.com/apikey' },
      { en: 'Press "Create API key".', zh: '按「Create API key」建立密鑰。' },
      {
        en: 'Turn on billing for the project. A key without billing is used for chat only, never for mail or calendar.',
        zh: '為專案開啟帳單。未開啟帳單的密鑰只會用於聊天，不會用於郵件或日曆。',
        link: 'https://ai.google.dev/gemini-api/docs/billing',
      },
      pasteStep,
    ],
  },
  {
    id: 'openai',
    steps: [
      { en: 'Open the OpenAI API keys page and sign in.', zh: '開啟 OpenAI 的 API 密鑰頁面並登入。', link: 'https://platform.openai.com/api-keys' },
      { en: 'Create a new secret key and copy it.', zh: '建立新的密鑰並複製。' },
      pasteStep,
    ],
  },
  {
    id: 'claude',
    steps: [
      { en: 'Open the Anthropic Console keys page and sign in.', zh: '開啟 Anthropic Console 的密鑰頁面並登入。', link: 'https://console.anthropic.com/settings/keys' },
      { en: 'Create a new API key and copy it.', zh: '建立新的 API 密鑰並複製。' },
      pasteStep,
    ],
  },
  {
    id: 'kimi',
    steps: [
      { en: 'Open the Moonshot AI platform key page and sign in.', zh: '開啟 Moonshot AI 平台的密鑰頁面並登入。', link: 'https://platform.moonshot.ai/console/api-keys' },
      { en: 'Create a new API key and copy it.', zh: '建立新的 API 密鑰並複製。' },
      pasteStep,
    ],
  },
  {
    id: 'qwen',
    steps: [
      { en: 'Open the Alibaba Cloud Model Studio guide to getting an API key and sign in.', zh: '開啟阿里雲百煉（Model Studio）的取得密鑰說明並登入。', link: 'https://www.alibabacloud.com/help/en/model-studio/get-api-key' },
      { en: 'Create a new API key and copy it.', zh: '建立新的 API 密鑰並複製。' },
      pasteStep,
    ],
  },
];

/**
 * One provider's guide with its key link taken from the registry: the setup
 * entry's `key_page`, else the global region of the compatible service of the
 * same id (Kimi and Qwen keep theirs there), else the link above.
 */
export function guideFor(id: ProviderGuide['id'], setup: SetupChoice[], compatible: CompatibleService[] = []): ProviderGuide {
  const guide = PROVIDER_GUIDES.find(entry => entry.id === id);
  if (!guide) throw new Error(`no guide for provider ${id}`);
  const service = compatible.find(entry => entry.id === id);
  const keyPage = setup.find(choice => choice.id === id)?.key_page
    ?? (service?.regions.find(region => region.id === 'global') ?? service?.regions[0])?.key_page
    ?? null;
  if (!keyPage) return guide;
  return { ...guide, steps: guide.steps.map((step, index) => (index === 0 ? { ...step, link: keyPage } : step)) };
}
