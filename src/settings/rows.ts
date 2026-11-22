import type { SettingsSectionId } from './sections.ts';

// Every row of every settings section, declared once, in display order. The
// dialog searches this list, so a section that is not mounted can still be
// found. A section reads its titles from here with rowText(ROWS_BY_ID[id]).
// Removed on purpose: "Display name" (the sidebar reads assistant_address,
// then display_name, then user_name), the second speech-model download button,
// and the duplicate provider key field.

export interface RowMeta {
  id: string;
  section: SettingsSectionId;
  title: [en: string, zh: string];
  description?: [en: string, zh: string];
  keywords?: string[];
}

function row(
  id: string,
  section: SettingsSectionId,
  title: [string, string],
  description?: [string, string],
  keywords?: string[],
): RowMeta {
  const meta: RowMeta = { id, section, title };
  if (description) meta.description = description;
  if (keywords) meta.keywords = keywords;
  return meta;
}

export const ROWS: RowMeta[] = [
  // General
  row('account', 'general', ['Account', '帳戶'], ['Signed in, plan, billing and sign out.', '登入狀態、方案、帳單及登出。'], ['sign in', 'sign out', 'plan', 'billing', 'subscription', 'pro']),
  row('language', 'general', ['Language', '語言'], ['The language of menus and buttons.', '選單與按鈕使用的語言。'], ['interface', 'locale', 'english', '繁體中文']),
  row('reply-language', 'general', ['JARVIS replies in', 'JARVIS 回覆語言'], ['English, written Chinese or both.', '英文、繁體書面語或雙語。'], ['response language', 'bilingual']),
  row('chinese-replies', 'general', ['Chinese replies', '中文回覆'], ['Mail, events and notes for other people always use written Chinese.', '寫給他人的郵件、活動及筆記一律使用書面語。'], ['written chinese', 'cantonese', '廣東話']),
  row('theme', 'general', ['Theme', '主題'], ['Light, dark or follow the system.', '淺色、深色或跟隨系統。'], ['appearance', 'colour', 'color', 'dark mode', 'light mode']),
  row('assistant-address', 'general', ['What JARVIS calls you', 'JARVIS 對你的稱呼'], ['Used in replies and in the greeting.', '用於回覆及問候語。'], ['name', 'address', 'nickname']),
  row('assistant-tone', 'general', ['Tone', '語氣'], undefined, ['persona', 'style', 'professional', 'friendly']),
  row('assistant-length', 'general', ['Reply length', '回覆長度'], undefined, ['brief', 'detailed', 'balanced', 'persona']),
  row('assistant-search', 'general', ['Search depth', '搜尋深度'], undefined, ['thorough', 'quick', 'persona', 'web']),
  row('assistant-instructions', 'general', ['Anything else about how you like replies', '其他回覆偏好'], ['Free text added to every conversation.', '會加入每次對話的自由文字。'], ['custom instructions', 'persona', 'prompt']),
  row('wallpaper', 'general', ['Wallpaper mode', '桌布模式'], ['Show JARVIS as the desktop background.', '將 JARVIS 顯示為桌面背景。'], ['desktop', 'background', 'experimental']),
  row('open-at-login', 'general', ['Open at login', '登入時開啟'], undefined, ['startup', 'launch', 'boot']),
  row('keep-awake', 'general', ['Keep awake on mains power', '接駁電源時保持喚醒'], ['Stops the Mac sleeping while plugged in, so background work continues.', '接駁電源時避免 Mac 進入睡眠，讓背景工作繼續。'], ['sleep', 'caffeinate', 'power', 'battery']),

  // Your AI
  row('ai-current', 'ai', ['Now using', '目前使用'], ['The AI that answers chat, mail and calendar questions.', '負責回答對話、郵件及日曆問題的 AI。'], ['provider', 'model', 'chosen']),
  row('ai-consent', 'ai', ['Data terms for this AI', '此 AI 的資料條款'], ['Agree before your text is sent to the service.', '同意後，你的文字才會傳送到該服務。'], ['consent', 'agree', 'terms']),
  row('ai-google-note', 'ai', ['What Google sees', 'Google 會看到甚麼'], ['Gmail and Calendar content is not sent to this AI unless you ask.', '除非你要求，否則 Gmail 與日曆內容不會傳送給此 AI。'], ['gmail', 'calendar', 'privacy']),
  row('ai-choice', 'ai', ['Choose your AI', '選擇你的 AI'], ['Google Gemini, on this Mac, or another service.', 'Google Gemini、本機執行或其他服務。'], ['gemini', 'claude', 'openai', 'ollama', 'kimi', 'qwen', 'provider']),
  row('ai-region', 'ai', ['Account region', '帳戶地區'], ['Kimi and Qwen: International or Mainland China.', 'Kimi 與 Qwen：國際版或中國大陸版。'], ['kimi', 'qwen', 'international', 'mainland']),
  row('ai-key', 'ai', ['API key', 'API 金鑰'], ['Paste the key for the service you chose.', '貼上你所選服務的金鑰。'], ['secret', 'token', 'get a key', 'paste']),
  row('ai-model', 'ai', ['Model', '模型'], ['Load the models your key can use, then pick one.', '載入你的金鑰可用的模型，再選擇一個。'], ['load models', 'latest', 'recommended']),
  row('ai-ollama', 'ai', ['Ollama on this Mac', '本機 Ollama'], ['Free and private. Needs Ollama running.', '免費且私密，需要 Ollama 正在執行。'], ['local', 'download model', 'offline']),
  row('ai-test-consent', 'ai', ['Allow a test message', '允許傳送測試訊息'], ['Testing sends a short message to the service.', '測試時會向該服務傳送一則短訊息。'], ['consent', 'test']),
  row('ai-test-use', 'ai', ['Test and use this AI', '測試並使用此 AI'], ['Replaces your provider order and mail processing mode.', '會取代你的供應商順序及郵件處理模式。'], ['save', 'apply', 'choose']),
  row('ai-mail-link', 'ai', ['Mail analysis settings', '郵件分析設定'], ['Opens Mail and follow-ups.', '開啟「郵件與跟進」。'], ['mail', 'analysis']),

  // Usage
  row('usage-period', 'usage', ['Period', '期間'], ['30 days, 90 days or 1 year.', '30 日、90 日或 1 年。'], ['range', 'days', 'history']),
  row('usage-spent', 'usage', ['Spent', '已花費'], ['US$, with HK$ beside it.', '美元，旁附港元。'], ['cost', 'money', 'price', 'bill', 'hkd']),
  row('usage-mail-requests', 'usage', ['AI requests for mail', '郵件的 AI 請求'], undefined, ['mail sorting', 'triage', 'count']),
  row('usage-chat-requests', 'usage', ['AI requests in chat', '對話中的 AI 請求'], undefined, ['chat', 'voice', 'tools', 'count']),
  row('usage-daily', 'usage', ['Cost per day', '每日費用'], undefined, ['chart', 'daily', 'bar']),
  row('usage-breakdown', 'usage', ['Where it went', '費用去向'], ['Cost by feature.', '按功能劃分的費用。'], ['feature', 'chart', 'group by']),
  row('usage-unpriced', 'usage', ['Requests with no price', '沒有價格的請求'], ['Counted, but not added to the money total.', '會計入數量，但不計入總費用。'], ['unpriced', 'free', 'local']),
  row('usage-details', 'usage', ['Show details', '顯示詳情'], ['Per-request table and the most expensive requests.', '逐項請求表及費用最高的請求。'], ['table', 'tokens', 'expensive', 'group by', '180 days', '2 years']),

  // Mail and follow-ups
  row('mail-auto-sort', 'mail', ['Sort new mail with AI', '以 AI 整理新郵件'], ['Finds replies you owe and follow-ups. Recommended.', '找出需要回覆的郵件及跟進事項。建議開啟。'], ['triage', 'automatic', 'recommended', 'typesafe']),
  row('mail-triage-consent', 'mail', ['Email text is sent to the AI', '郵件文字會傳送給 AI'], ['Tick to confirm you understand before sorting turns on.', '開啟整理前，請勾選以確認你已了解。'], ['consent', 'privacy']),
  row('mail-ai-status', 'mail', ['Which AI sorts your mail', '由哪個 AI 整理郵件'], ['Shows "No AI chosen yet" until you pick one in Your AI.', '在「你的 AI」選定前，會顯示「尚未選擇 AI」。'], ['status', 'sorting']),
  row('mail-processing', 'mail', ['Processing', '處理方式'], ['Same AI as the assistant, Ollama on this Mac, OpenAI or your AWS Bedrock account.', '與助理相同的 AI、本機 Ollama、OpenAI 或你的 AWS Bedrock 帳戶。'], ['provider', 'ollama', 'openai', 'bedrock', 'analysis']),
  row('mail-analysis-consent', 'mail', ['Group analysis consent', '群組分析同意'], ['Analysis sends the text of grouped emails to the AI.', '分析會將歸組郵件的文字傳送給 AI。'], ['consent', 'privacy', 'group']),

  // Daily briefing
  row('briefing-enabled', 'briefing', ['Daily briefing', '每日簡報'], ['A morning summary of mail and calendar.', '每朝的郵件與日曆摘要。'], ['morning', 'summary', 'schedule']),
  row('briefing-time', 'briefing', ['Briefing time', '簡報時間'], ['Saved when you leave the field or press Enter.', '離開欄位或按 Enter 後儲存。'], ['clock', 'schedule', 'morning']),
  row('briefing-aloud', 'briefing', ['Read the briefing aloud', '朗讀簡報'], ['Needs text-to-speech turned on in Voice.', '需要在「語音」開啟語音朗讀。'], ['tts', 'speak', 'voice']),

  // Voice
  row('stt-language', 'voice', ['Speech-to-text language', '語音輸入語言'], ['Auto-detect, English or Cantonese.', '自動偵測、英文或廣東話。'], ['stt', 'dictation', 'microphone', 'cantonese', '廣東話', 'language']),
  row('voice-input-status', 'voice', ['Voice input', '語音輸入'], ['Cmd+Shift+J starts and stops listening.', '按 Cmd+Shift+J 開始或停止聆聽。'], ['shortcut', 'hotkey', 'microphone', 'status']),
  row('speech-model', 'voice', ['Speech model', '語音模型'], ['About 148 MB, used when cloud speech is not available.', '約 148 MB，在無法使用雲端語音時使用。'], ['whisper', 'download', 'offline', 'stt']),
  row('tts-enabled', 'voice', ['Enable text-to-speech', '啟用語音朗讀'], undefined, ['tts', 'speak', 'read aloud', 'voice replies']),
  row('keep-recordings', 'voice', ['Keep recordings of my voice requests', '保留我的語音請求錄音'], ['Turning this off deletes kept recordings.', '關閉後會刪除已保留的錄音。'], ['audio', 'privacy', 'delete']),
  row('tts-voice', 'voice', ['Voice', '聲音'], undefined, ['tts', 'english', 'speaker']),
  row('tts-voice-yue', 'voice', ['Cantonese voice', '廣東話聲音'], undefined, ['tts', 'cantonese', '廣東話', 'speaker']),
  row('wake-word', 'voice', ['Enable wake word', '啟用喚醒詞'], ['Experimental. Listens for "Hey JARVIS" on this Mac.', '實驗功能。在此 Mac 上聆聽喚醒詞。'], ['always listening', 'hey jarvis', 'experimental', 'hands free']),
  row('wake-status', 'voice', ['Wake word status', '喚醒詞狀態'], ['Listening, downloading or ready.', '聆聽中、下載中或已就緒。'], ['model', 'progress', 'error']),
  row('wake-privacy', 'voice', ['Where wake-word audio goes', '喚醒詞音訊的去向'], undefined, ['privacy', 'audio', 'microphone']),

  // Connections
  row('google-account', 'connections', ['Google account', 'Google 帳戶'], ['The main account for Gmail and Calendar.', 'Gmail 與日曆使用的主要帳戶。'], ['gmail', 'calendar', 'connect', 'reconnect', 'sign in']),
  row('google-gmail', 'connections', ['Gmail', 'Gmail'], ['On or off for this account.', '此帳戶的開啟或關閉狀態。'], ['mail', 'connect gmail']),
  row('google-calendar', 'connections', ['Google Calendar', 'Google 日曆'], ['Allow JARVIS to read and add events.', '允許 JARVIS 讀取及新增活動。'], ['allow calendar', 'events', 'permission']),
  row('google-switch', 'connections', ['Switch Google account', '切換 Google 帳戶'], ['Removes the old account\'s cached mail, events and briefings.', '會移除舊帳戶已儲存的郵件、活動及簡報。'], ['change account', 'revoke']),
  row('google-disconnect', 'connections', ['Disconnect Google', '中斷 Google 連接'], undefined, ['remove', 'revoke', 'sign out']),
  row('google-other-accounts', 'connections', ['Other Google accounts', '其他 Google 帳戶'], ['Mail and calendar, mail only, or reconnect needed.', '郵件與日曆、僅郵件，或需要重新連接。'], ['mailbox', 'extra account', 'allow calendar', 'disconnect', 'remove', '中斷連接', '移除']),
  row('google-add-account', 'connections', ['Add another Google account', '新增另一個 Google 帳戶'], undefined, ['mailbox', 'second account']),
  row('notion', 'connections', ['Notion', 'Notion'], ['Status and one action.', '狀態及主要操作。'], ['notes', 'connect', 'disconnect', 'workspace']),
  row('obsidian', 'connections', ['Obsidian', 'Obsidian'], ['Select your vault and allow access.', '選擇你的 Vault 並允許存取。'], ['notes', 'vault', 'connect', 'disconnect']),
  row('github', 'connections', ['GitHub', 'GitHub'], ['Status and one action.', '狀態及主要操作。'], ['code', 'issues', 'connect', 'disconnect', 'device code']),

  // Privacy and data
  row('ai-consents', 'privacy', ['Data terms you agreed to', '你已同意的資料條款'], ['Withdraw any of them here.', '可在此撤回任何一項。'], ['consent', 'withdraw', 'revoke', 'agreement']),
  row('storage-usage', 'privacy', ['On this Mac', '在此 Mac 上'], ['Database, models, web cache, old backups and counts.', '資料庫、模型、網頁快取、舊備份及數量。'], ['storage', 'disk', 'size', 'gb', 'data folder']),
  row('retention-enabled', 'privacy', ['Remove content older than', '移除早於指定時間的內容'], ['Automatic clean-up at launch.', '於啟動時自動清理。'], ['retention', 'auto delete', 'clean-up']),
  row('retention-window', 'privacy', ['Keep content for', '內容保留期限'], ['30 days, 90 days, 180 days or 1 year.', '30 日、90 日、180 日或 1 年。'], ['retention', 'window', 'days']),
  row('cleanup-now', 'privacy', ['Clean up now', '立即清理'], undefined, ['sweep', 'delete old', 'retention']),
  row('purge-cache', 'privacy', ['Clear web cache', '清除網頁快取'], ['Rebuilt automatically.', '會自動重建。'], ['delete', 'cache', 'storage']),
  row('purge-backups', 'privacy', ['Delete old backups', '刪除舊備份'], ['Permanent.', '無法復原。'], ['delete', 'backup', 'storage']),
  row('purge-models', 'privacy', ['Delete downloaded models', '刪除已下載的模型'], undefined, ['delete', 'whisper', 'ollama', 'storage']),
  row('purge-mail-bodies', 'privacy', ['Drop stored mail text', '移除已儲存的郵件內文'], undefined, ['delete', 'email', 'storage']),
  row('purge-conversations', 'privacy', ['Delete all chat history', '刪除所有對話記錄'], ['Cannot be undone.', '無法復原。'], ['delete', 'conversations', 'chat', 'storage']),
  row('delete-everything', 'privacy', ['Delete everything and restart', '刪除全部資料並重新啟動'], ['Type DELETE to confirm. Also removes Keychain items and revokes Google.', '須輸入 DELETE 確認。同時會移除鑰匙圈項目並撤銷 Google 授權。'], ['erase', 'reset', 'wipe', 'keychain', 'factory']),

  // Setup guide
  row('setup-checklist', 'setup', ['Setup checklist', '設定清單'], ['AI ready, Google connected, mail sorting on, briefing time set.', 'AI 已就緒、Google 已連接、郵件整理已開啟、已設定簡報時間。'], ['status', 'onboarding', 'progress']),
  row('setup-open', 'setup', ['Open setup guide', '開啟設定導引'], ['Walks through the first-run steps again.', '重新引導你完成首次設定。'], ['onboarding', 'wizard', 'first run', 'quick setup']),

  // About
  row('about-version', 'about', ['About JARVIS', '關於 JARVIS'], ['Version, model list version and update check.', '版本、模型清單版本及更新檢查。'], ['version', 'update', 'registry', 'model list']),

  // Advanced: Models
  row('chain-order', 'advanced-models', ['Provider priority', '供應商優先次序'], ['The order JARVIS tries providers in. Move, turn off or remove entries.', 'JARVIS 嘗試供應商的先後次序。可移動、停用或移除項目。'], ['chain', 'fallback', 'order', 'enable', 'remove']),
  row('chain-add', 'advanced-models', ['Add to priority', '加入優先次序'], ['Add Gemini, Claude, OpenAI or Bedrock to the order.', '將 Gemini、Claude、OpenAI 或 Bedrock 加入次序。'], ['chain', 'gemini pro', 'provider']),
  row('chain-bedrock-model', 'advanced-models', ['Bedrock model', 'Bedrock 模型'], ['Pick from the list AWS returns for your region, or enter a model ID.', '從 AWS 為你的地區提供的清單中選擇，或輸入模型 ID。'], ['aws', 'model id', 'load the list', 'inference profile']),
  row('default-provider', 'advanced-models', ['Default provider', '預設供應商'], ['Used only when no provider order is saved.', '只在沒有儲存供應商次序時使用。'], ['ai_provider', 'gemini', 'fallback']),
  row('ai-base-url', 'advanced-models', ['API base URL', 'API 基本網址'], ['For "Other" services that speak the OpenAI format.', '適用於採用 OpenAI 格式的「其他」服務。'], ['endpoint', 'custom', 'compatible']),
  row('ai-model-choose', 'advanced-models', ['Choose a model', '選擇模型'], ['Optional. Latest (recommended) if left alone.', '可選。不選擇時使用「最新（建議）」。'], ['latest', 'model list', 'expensive']),
  row('mail-local-model', 'advanced-models', ['Mail: Ollama model name', '郵件：Ollama 模型名稱'], undefined, ['mail', 'local', 'ollama']),
  row('mail-openai-model', 'advanced-models', ['Mail: OpenAI model', '郵件：OpenAI 模型'], ['Shows the price per million tokens.', '顯示每百萬個 token 的價格。'], ['mail', 'openai', 'price']),
  row('mail-bedrock-model', 'advanced-models', ['Mail: Bedrock model', '郵件：Bedrock 模型'], undefined, ['mail', 'aws', 'bedrock']),

  // Advanced: Cloud providers
  row('key-gemini', 'advanced-cloud', ['Google Gemini API key', 'Google Gemini API 金鑰'], ['Stored in the Keychain.', '儲存於鑰匙圈。'], ['api key', 'secret', 'remove', 'gemini']),
  row('key-claude', 'advanced-cloud', ['Anthropic Claude API key', 'Anthropic Claude API 金鑰'], ['Stored in the Keychain.', '儲存於鑰匙圈。'], ['api key', 'secret', 'remove', 'anthropic']),
  row('key-openai', 'advanced-cloud', ['OpenAI API key', 'OpenAI API 金鑰'], ['Stored in the Keychain.', '儲存於鑰匙圈。'], ['api key', 'secret', 'remove', 'gpt']),
  row('key-typesafe', 'advanced-cloud', ['TypeSafe API key', 'TypeSafe API 金鑰'], ['Optional. Cheaper mail sorting.', '可選。令郵件整理成本更低。'], ['typesafe', 'mail', 'sorting', 'api key', 'remove', '移除']),
  row('aws-access-key', 'advanced-cloud', ['AWS Access Key ID', 'AWS Access Key ID'], ['For Bedrock. Stored in the Keychain.', '供 Bedrock 使用，儲存於鑰匙圈。'], ['bedrock', 'credentials']),
  row('aws-secret-key', 'advanced-cloud', ['AWS Secret Access Key', 'AWS Secret Access Key'], undefined, ['bedrock', 'credentials', 'remove key']),
  row('aws-region', 'advanced-cloud', ['AWS region', 'AWS 地區'], ['Where Bedrock requests run.', 'Bedrock 請求執行的地區。'], ['bedrock', 'singapore', 'tokyo', 'london', 'us east']),

  // Advanced: Local AI
  row('local-system', 'advanced-local', ['This Mac', '此 Mac'], ['RAM, GPU and a suggested model size.', '記憶體、GPU 及建議的模型大小。'], ['compatibility', 'hardware', 'ram', 'gpu']),
  row('local-backend', 'advanced-local', ['Local backend', '本機後端'], ['Ollama, vLLM or generic. Checks what is installed.', 'Ollama、vLLM 或通用後端。檢查已安裝的項目。'], ['ollama', 'vllm', 'prerequisites', 'refresh']),
  row('local-models', 'advanced-local', ['Recommended models', '建議模型'], ['Pull a model, see progress, or add it to the order.', '下載模型、查看進度，或加入次序。'], ['pull', 'download', 'cancel', 'chain']),
  row('local-endpoint-url', 'advanced-local', ['vLLM or generic URL', 'vLLM 或通用網址'], undefined, ['connect', 'endpoint', 'server']),
  row('local-endpoints', 'advanced-local', ['Local model endpoints', '本機模型端點'], ['Health, latency, models and tool support. Test, add to priority or remove.', '健康狀態、延遲、模型及工具支援。可測試、加入次序或移除。'], ['health', 'latency', 'test connection', 'remove']),
  row('local-endpoint-add', 'advanced-local', ['Add an endpoint', '新增端點'], ['A name (optional) and a URL.', '名稱（可選）及網址。'], ['name', 'url', 'server']),
  row('local-mail-setup', 'advanced-local', ['Local mail setup', '本機郵件設定'], ['Check this Mac, download a model, and use it for mail.', '檢查此 Mac、下載模型，並用於郵件。'], ['ollama', 'install', 'prepare', 'model to download', 'disk']),

  // Advanced: Integrations
  row('github-client-id', 'advanced-integrations', ['GitHub OAuth Client ID', 'GitHub OAuth Client ID'], ['Register a GitHub application, then save its Client ID.', '註冊 GitHub 應用程式後，儲存其 Client ID。'], ['oauth', 'application', 'register']),
  row('notion-token', 'advanced-integrations', ['Notion integration token', 'Notion 整合權杖'], undefined, ['secret', 'api key', 'notion']),
  row('github-token', 'advanced-integrations', ['GitHub personal access token', 'GitHub 個人存取權杖'], undefined, ['secret', 'pat', 'github']),
  row('obsidian-key', 'advanced-integrations', ['Obsidian API key', 'Obsidian API 金鑰'], ['From the Local REST API plugin.', '來自 Local REST API 外掛程式。'], ['secret', 'rest api', 'obsidian']),
  row('obsidian-endpoint', 'advanced-integrations', ['Obsidian endpoint', 'Obsidian 端點'], ['Saved when you leave the field.', '離開欄位後儲存。'], ['base url', 'address', 'port']),

  // Advanced: Developer
  row('ai-error-log', 'advanced-developer', ['Last AI error', '最近的 AI 錯誤'], undefined, ['log', 'diagnostics', 'llm error', 'debug']),
  row('speech-model-redownload', 'advanced-developer', ['Download the speech model again', '重新下載語音模型'], undefined, ['whisper', 'repair', 'redownload']),
  row('speech-model-path', 'advanced-developer', ['Speech model file', '語音模型檔案'], ['Where the model is stored on disk.', '模型儲存於磁碟的位置。'], ['path', 'whisper', 'folder', 'application support']),
];

export const ROWS_BY_ID: Record<string, RowMeta> = Object.fromEntries(ROWS.map((meta) => [meta.id, meta]));

export function rowText(meta: RowMeta, locale: 'en' | 'zh-HK'): { title: string; description?: string } {
  const index = locale === 'zh-HK' ? 1 : 0;
  const text: { title: string; description?: string } = { title: meta.title[index] };
  if (meta.description) text.description = meta.description[index];
  return text;
}
