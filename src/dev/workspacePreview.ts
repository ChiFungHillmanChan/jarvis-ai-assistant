// Development-only, synthetic data for inspecting the actual app without services.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import type { ChatMessage } from '../lib/types';
export function installWorkspacePreview() {
  mockWindows('main');
  const saved = JSON.parse(localStorage.getItem('jarvis-workspace-preview') || '{}') as Record<string, string>;
  const conversations: Record<string, ChatMessage[]> = JSON.parse(localStorage.getItem('jarvis-preview-conversations') || '{}');
  const now = new Date(); const day = now.toISOString().slice(0, 10);
  const mailSettings = { processing_mode: 'local', local_model: 'qwen3:8b', bedrock_model: '', openai_model: '' };
  const accounts = [{ id: 'sample-account', email: 'alex@example.com', connected: true, message_count: 4, last_synced: now.toISOString(), next_page_token: null, calendar: true }];
  const groups = [{ id: 1, name: 'Customer follow-ups', instructions: 'Summarize the decisions and follow-ups.', message_count: 2 }];
  const shift = (days: number) => { const d = new Date(now); d.setDate(d.getDate() + days); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; };
  // Synthetic mail with the triage shape the backend returns (mail/triage.rs).
  const samples: [string, string, string, string, number, boolean, string | null, string | null][] = [
    ['September project review', 'Sam Taylor <sam@example.com>', 'work', 'Sam 需要你在週五前確認檢討時間表，並就建議書提出意見。', 2.4, true, shift(2), 'Could we review the timeline on Friday?'],
    ['Proposal: next steps', 'Jordan Lee <jordan@example.com>', 'work', 'Jordan 已附上建議書，等待你回覆是否同意報價。', 2.1, true, null, 'Please let me know if the quote works for you.'],
    ['Interview invitation: Product Engineer', 'Talent Team <talent@acme.example>', 'jobs', '邀請你於下週二下午 3 時進行 45 分鐘視像面試，請確認時段。', 2.6, true, shift(4), 'Tuesday at 3:00 PM'],
    ['Your invoice INV-2041 is due', 'Billing <billing@cloud.example>', 'finance', '雲端服務帳單 US$48.00 將於下月 1 日到期，會自動扣款。', 1.2, false, null, null],
    ['Booking confirmed: Leeds to Manchester', 'Rail Tickets <tickets@rail.example>', 'schedule', '已確認下週一 12:15 由 Leeds 前往 Manchester 的車票。', 1.4, false, shift(6), 'Outbound Monday 12:15 Leeds to Manchester'],
    ['New sign-in to your account', 'Security <no-reply@service.example>', 'account', null as unknown as string, 0.8, false, null, null],
    ['Weekly product update', 'Product News <news@startup.example>', 'promotions', null as unknown as string, 0.2, false, null, null],
    ['晚飯時間', 'Mum <mum@example.hk>', 'personal', '媽媽問你星期日是否回家吃晚飯。', 1.7, true, null, '星期日返唔返嚟食飯？'],
  ];
  const messages = samples.map(([subject, sender, category, summary, importance, attention, due], index) => ({ id: index + 1, account_id: 'sample-account', account_email: 'alex@example.com', provider_id: `sample${index}`, thread_id: null, subject, sender, snippet: 'Here are the details for our next review. Please check the timeline and proposed next steps.', body: `Hi Alex,\n\nHere are the details for our next review. The proposal is ready for your feedback.\n\nCould we review the timeline on Friday?\n\nThanks,\nSam`, is_read: index > 2, is_inbox: true, received_at: new Date(now.getTime() - index * 5 * 3600_000).toISOString(), attachments: index === 0 ? [{ filename: 'Review timeline.pdf', mime_type: 'application/pdf', size: 184320 }] : index === 1 ? [] : null, next_due: due, triage: { category, category_confidence: index === 3 ? 0.42 : 0.86, needs_reply: attention ? 0.9 : 0.05, action_required: attention ? 0.85 : 0.2, has_date: due ? 0.9 : 0.1, importance, sent_by_owner: false, attention, summary: summary || null, decided_by: index === 3 ? 'luna' : 'jev' } }));
  let mailItems = samples.flatMap(([subject, sender, , , , , due, evidence], index) => evidence ? [{ id: index + 1, message_id: index + 1, kind: index === 1 || index === 7 ? 'reply' : index === 4 ? 'event' : index === 2 ? 'event' : 'deadline', title: index === 0 ? '確認檢討時間表' : index === 1 ? '回覆報價' : index === 2 ? '產品工程師面試' : index === 4 ? '乘火車前往 Manchester' : '回覆媽媽', due_date: due, due_time: index === 2 ? '15:00' : index === 4 ? '12:15' : null, end_time: index === 2 ? '15:45' : null, location: null, evidence, evidence_verified: index !== 2, status: 'open', remind_at: null as string | null, reminded_at: null, calendar_event_id: null as string | null, subject, sender }] : []);
  const triageStatus = () => ({ enabled: true, typesafe_key: true, openai_key: true, chosen_ai: "OpenAI", untriaged: 3, attention: messages.filter(m => m.triage.attention).length, open_items: mailItems.filter(i => i.status === 'open').length, categories: ['work', 'jobs', 'finance', 'schedule', 'account', 'personal', 'promotions'].map(c => [c, messages.filter(m => m.triage.category === c).length]) });
  let tasks = [['Reply to the proposal', -1], ['Send the review notes', 0], ['Confirm Friday planning', 2], ['Quarterly review', 10]].map(([title, days], index) => ({ id: index + 1, title: title as string, description: null, deadline: shift(days as number), priority: 1, status: 'pending', source: 'manual', created_at: null }));
  const events = [{ id: 1, google_id: 'sample-event', summary: 'Customer onboarding review', description: 'Review the rollout plan and answer open questions.', location: 'Meeting room 2', start_time: day + 'T14:00:00Z', end_time: day + 'T14:45:00Z', attendees: '', status: 'confirmed', account: null }];
  // A realistic week around today for the Calendar views: overlaps, all-day, multi-day and cross-midnight.
  const localIso = (days: number, hour: number, minute = 0) => { const d = new Date(now); d.setDate(d.getDate() + days); d.setHours(hour, minute, 0, 0); return d.toISOString(); };
  const weekEvents = ([
    [-2, 9, 0, 10, 0, 'Team standup'], [-1, 10, 0, 11, 30, 'Investor prep'], [-1, 10, 30, 11, 0, 'Call with Jordan'], [0, 9, 30, 10, 0, 'Standup'],
    [0, 11, 0, 12, 30, 'Evoke pilot check-in'], [0, 16, 0, 17, 0, 'HKSTP mentor session'], [1, 13, 0, 14, 0, 'Lunch with Sam'], [2, 22, 0, 25, 30, 'Overnight deploy'],
    [3, 15, 0, 16, 30, 'Design review'], [5, 10, 0, 11, 0, 'Weekly planning'],
  ] as [number, number, number, number, number, string][]).map(([days, h1, m1, h2, m2, summary], index) => ({ id: 100 + index, google_id: `sample-week-${index}`, summary, description: null, location: index === 4 ? 'Cyberport' : null, start_time: localIso(days, h1, m1), end_time: localIso(days, h2, m2), attendees: '', status: 'confirmed', account: null }));
  weekEvents.push({ id: 120, google_id: 'sample-allday', summary: 'Public holiday', description: null, location: null, start_time: shift(1), end_time: shift(2), attendees: '', status: 'confirmed', account: null });
  weekEvents.push({ id: 121, google_id: 'sample-trip', summary: 'Trip to Shenzhen', description: null, location: null, start_time: shift(-3), end_time: shift(0), attendees: '', status: 'confirmed', account: null });
  const github = [{ id: 1, title: 'Improve onboarding checklist', repo: 'example/workspace', number: 42, state: 'open', item_type: 'issue', url: 'https://github.com/example/workspace/issues/42', updated_at: now.toISOString() }];
  const notes = [{ id: 1, notion_id: '12345678901234567890123456789012ab', title: 'Customer onboarding playbook', url: 'https://www.notion.so/12345678901234567890123456789012ab', parent_type: 'workspace', last_edited: now.toISOString() }];
  const jobs = [{ id: 1, name: 'Morning briefing', schedule: '0 0 9 * * MON-FRI', action_type: 'briefing', status: 'active', last_run: day + ' 09:00:00', next_run: null, description: 'Review my calendar every weekday at 9 am.' }, { id: 2, name: 'Weekly project review', schedule: '0 0 16 * * FRI', action_type: 'summary', status: 'paused', last_run: null, next_run: null, description: 'Summarize project progress on Friday.' }];
  const runs = [{ id: 1, job_id: 1, started_at: day + ' 09:00:00', finished_at: day + ' 09:00:12', status: 'success', result: 'Your calendar has one customer review today.\n\nPrepare the rollout plan before the meeting.', error: null }];
  let wakeOn = false;
  let openAtLogin = false, keepAwake = false;
  let briefing = { enabled: false, time: '08:30', voice: true, speech_enabled: false };
  // Settings > Connections, Privacy and data, Setup guide and About (Task 7).
  let consents = [{ provider: 'openai', base_url: '', label: 'OpenAI', terms: 'needs_billing_confirmation', terms_now: 'needs_billing_confirmation', registry_version: 7, consented_at: day + 'T09:00:00Z', holds: true }];
  const integrations: Record<string, { status: string; identity: string | null; method: string | null; message: string | null; oauth_ready?: boolean }> = {
    notion: { status: 'connected', identity: 'Alex workspace', method: 'mcp', message: null },
    github: { status: 'not_connected', identity: null, method: null, message: null, oauth_ready: true },
    obsidian: { status: 'not_connected', identity: null, method: null, message: null },
  };
  const storage = { databaseBytes: 48_200_000, walBytes: 1_200_000, modelsBytes: 148_000_000, cacheBytes: 12_400_000, backupsBytes: 96_000_000, totalBytes: 305_800_000, mailMessageCount: 412, mailBodyBytes: 8_600_000, conversationCount: 37, retentionDays: 90, retentionEnabled: true, dataDir: '/Users/alex/Library/Application Support/jarvis' };
  // Settings > Your AI (Task 10). Sample ids, never a real model id; prices are the registry's.
  const sampleModel = (provider: string, id: string, label: string, input: number, output: number) => ({ provider, id, label, family: false, status: 'active', max_output_tokens: null, price: { input_per_million: input, output_per_million: output, cache_read_per_million: 0, cache_write_per_million: 0 } });
  const sampleSetup = (id: string, en: string, model: string, keyPage: string | null) => ({ id, name: { en, 'zh-HK': en }, hint: { en: `Sample ${en} hint.`, 'zh-HK': `${en} 示範說明。` }, key_page: keyPage, model });
  const catalog = {
    defaults: { openai: 'sample-luna', claude: 'sample-haiku', gemini_flash: 'sample-flash', gemini_pro: '', gemini_mail_sort: 'sample-lite', bedrock: '', whisper: '' },
    cloud: [sampleModel('gemini', 'sample-lite', 'Gemini Flash-Lite (sample)', 0.25, 1.5), sampleModel('gemini', 'sample-flash', 'Gemini Flash (sample)', 0.75, 3.75), sampleModel('gemini', 'sample-smart', 'Gemini Smart (sample)', 1.5, 7.5), sampleModel('gemini', 'sample-pro', 'Gemini Pro (sample)', 2, 12), sampleModel('openai', 'sample-luna', 'GPT Luna (sample)', 0.1, 0.5), sampleModel('claude', 'sample-haiku', 'Claude Haiku (sample)', 0.1, 0.5)],
    local: [], registry: { version: 7, issued_at: day + 'T00:00:00Z', source: 'bundled' }, registry_updates: false, app_version: '0.5.13 (preview)',
    presets: [{ id: 'sample_primary', label: { en: 'Gemini (sample preset)', 'zh-HK': 'Gemini（示範預設）' } }, { id: 'sample_local', label: { en: 'Local first (sample preset)', 'zh-HK': '本機優先（示範預設）' } }], default_preset: 'sample_primary',
    setup: [sampleSetup('ollama', 'Ollama', 'required', null), sampleSetup('openai', 'OpenAI', 'optional', 'https://platform.openai.com/api-keys'), sampleSetup('gemini', 'Google Gemini', 'optional', 'https://aistudio.google.com/apikey'), sampleSetup('claude', 'Anthropic Claude', 'optional', 'https://console.anthropic.com/settings/keys'), sampleSetup('kimi', 'Kimi', 'required', null), sampleSetup('qwen', 'Qwen', 'required', null)],
    compatible: [
      { id: 'kimi', name: 'Kimi', regions: [{ id: 'global', base_url: 'https://api.moonshot.ai/v1', key_page: 'https://platform.moonshot.ai/console/api-keys' }, { id: 'china', base_url: 'https://api.moonshot.cn/v1', key_page: 'https://platform.moonshot.cn/console/api-keys' }], host_suffixes: ['moonshot.ai', 'moonshot.cn'] },
      { id: 'qwen', name: 'Qwen', regions: [{ id: 'global', base_url: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1', key_page: 'https://www.alibabacloud.com/help/en/model-studio/get-api-key' }], host_suffixes: ['aliyuncs.com'] },
    ],
  };
  // Settings > Advanced (Task 11). Nothing reaches a service; keys are never kept.
  let chain: { position: number; provider_type: string; endpoint_id?: string; model_id?: string; enabled: boolean; base_url?: string | null }[] = [{ position: 0, provider_type: 'openai', enabled: true }, { position: 1, provider_type: 'gemini', model_id: 'sample-flash', enabled: true }];
  let endpoints = [{ id: 'sample-ollama', name: 'Ollama', url: 'http://127.0.0.1:11434', backend_type: 'ollama', use_tls: false, connection_timeout_ms: 5000, keep_alive_minutes: 5, is_active: true, last_health_status: true }];
  const keyStatus: Record<string, boolean> = { gemini: false, claude: true, openai: true };
  let aws: { configured: boolean; region: string | null } = { configured: true, region: 'ap-southeast-1' };
  let currentAiValue: { provider: string; label: string; model: string | null; base_url: string | null } = { provider: 'openai', label: 'OpenAI', model: null, base_url: null };
  mockIPC(async (command, raw) => {
    const args = (raw || {}) as Record<string, any>;
    if (command.startsWith('plugin:')) return null;
    switch (command) {
      case 'check_onboarding_status': return true;
      case 'account_status': return { available: false, signed_in: false, email: null, tier: null, subscription_status: null, trial_ends_at: null, period_ends_at: null, cancel_at_period_end: false };
      case 'get_settings': return { values: { ai_provider: 'gemini_primary', google_email: 'alex@example.com', ...saved } };
      case 'update_setting': case 'set_user_preference': saved[args.key] = args.value; localStorage.setItem('jarvis-workspace-preview', JSON.stringify(saved)); return null;
      case 'get_user_preference': if (args.key in saved) return saved[args.key]; throw 'db read: Query returned no rows';
      case 'enable_wallpaper': case 'disable_wallpaper': return null;
      case 'get_open_at_login': return openAtLogin;
      case 'set_open_at_login': openAtLogin = args.enabled; return openAtLogin;
      case 'get_keep_awake': return { enabled: keepAwake, active: keepAwake };
      case 'set_keep_awake': keepAwake = args.enabled; return { enabled: keepAwake, active: keepAwake };
      case 'get_daily_briefing_settings': return briefing;
      case 'set_daily_briefing_settings': briefing = { ...briefing, enabled: args.enabled, time: args.time, voice: args.voice }; return briefing;
      case 'get_conversations': return conversations[args.sessionId || 'legacy'] || [];
      case 'list_chat_sessions': return Object.entries(conversations).map(([id, entries]) => ({ id, title: entries[0]?.content.slice(0, 60) || 'Task', updated_at: now.toISOString() }));
      case 'send_message': {
        const { sessionId, runId } = args; const scope = { session_id: sessionId, run_id: runId };
        const user: ChatMessage = { id: null, role: 'user', content: args.message, created_at: now.toISOString() };
        conversations[sessionId] = [...(conversations[sessionId] || []), user];
        await emit('chat-state', { ...scope, state: 'thinking' });
        await emit('chat-status', { ...scope, phase: 'acting', status: 'Running: Checking calendar...', detail: day });
        await new Promise(resolve => setTimeout(resolve, 1500));
        await emit('chat-status', { ...scope, phase: 'acting', status: 'Running: Checking follow-ups...', detail: null });
        await new Promise(resolve => setTimeout(resolve, 1000));
        await emit('chat-tool-result', { ...scope, tool_call_id: crypto.randomUUID(), tool_name: 'list_events', target: { view: 'calendar', accountId: 'legacy-google', dateRange: { from: day, to: day } }, result: { events } });
        const content = 'You have a customer onboarding review at 14:00 UTC. I opened the calendar result so you can review the details.';
        await emit('chat-token', { ...scope, token: content, done: false });
        await new Promise(resolve => setTimeout(resolve, 1200));
        await emit('chat-state', { ...scope, state: 'idle' });
        const answer: ChatMessage = { id: null, role: 'assistant', content, created_at: now.toISOString() };
        conversations[sessionId].push(answer); localStorage.setItem('jarvis-preview-conversations', JSON.stringify(conversations)); return answer;
      }
      case 'get_voice_state': return wakeOn ? 'WakeWordListening' : 'Idle';
      case 'clear_conversations': delete conversations[args.sessionId]; localStorage.setItem('jarvis-preview-conversations', JSON.stringify(conversations)); return null;
      case 'get_wallpaper_status': case 'is_wallpaper_raised': return false;
      case 'get_tasks': return tasks;
      case 'update_task': tasks = tasks.map(t => t.id === args.id ? { ...t, status: args.status } : t); return null;
      case 'get_emails': case 'mail_list_memories': return [];
      case 'get_events': case 'get_todays_events': return [...events, ...weekEvents];
      case 'get_github_items': return github;
      case 'get_notion_pages': return notes;
      case 'get_cron_jobs': return jobs.map(j => ({ ...j }));
      case 'get_cron_runs': return args.jobId === 1 ? runs : [];
      case 'get_upcoming_runs': return [day + 'T09:00:00Z'];
      case 'preview_custom_cron': return { name: 'Calendar sync', schedule: '0 0 9 * * MON-FRI', action_type: 'sync_calendar', description: args.description };
      case 'create_reviewed_cron': { const j = { id: jobs.length + 1, ...args.interpretation, status: 'active', last_run: null, next_run: null }; jobs.push(j); return j; }
      case 'set_cron_job_status': { const j = jobs.find(v => v.id === args.jobId); if (!j) throw 'Sample routine not found'; j.status = args.status; return j.status; }
      case 'toggle_cron_job': { const j = jobs.find(v => v.id === args.id); if (!j) throw 'Sample routine not found'; j.status = j.status === 'active' ? 'paused' : 'active'; return j.status; }
      case 'mail_workspace': return { accounts, groups, settings: mailSettings, triage: triageStatus() };
      case 'mail_list_messages': { const filtered = messages.filter(m => (!args.query || m.subject.toLowerCase().includes(args.query.toLowerCase())) && (!args.groupId || m.id <= 2) && (!args.view || (args.view === 'attention' ? m.triage.attention : args.view === 'untriaged' ? false : m.triage.category === args.view))); return { messages: filtered, total: args.query ? null : filtered.length, has_more: false }; }
      case 'mail_set_attention': { const m = messages.find(v => v.id === args.messageId); if (m) m.triage.attention = args.attention; return null; }
      case 'mail_list_items': return mailItems.filter(i => (args.messageId ? i.message_id === args.messageId : i.status === 'open'));
      case 'mail_update_item': { mailItems = mailItems.map(i => i.id === args.itemId ? { ...i, status: args.status, remind_at: args.remindAt ?? null } : i); return mailItems.find(i => i.id === args.itemId); }
      case 'mail_item_to_calendar': { mailItems = mailItems.map(i => i.id === args.itemId ? { ...i, calendar_event_id: 'preview' } : i); return mailItems.find(i => i.id === args.itemId); }
      case 'mail_triage_run': return { triaged: 3, summarized: 2, items: 1, failed: 0, remaining: 0, jev_input_tokens: 4200, luna_input_tokens: 2600, luna_output_tokens: 300, cost_usd: 0.0006, price_unknown: false, warning: null };
      case 'mail_get_message': return messages.find(m => m.id === args.messageId);
      case 'mail_analyze_group': return { text: 'Two customer conversations need follow-up.\n\n- Confirm the project review date.\n- Send feedback on the proposal.', mode: 'local', model: 'Preview only', source_ids: [1, 2], omitted_messages: 0 };
      case 'get_api_key_status': return { ...keyStatus };
      case 'set_api_key': if (args.provider === 'typesafe') return null; keyStatus[args.provider] = !args.remove; return null;
      case 'update_provider_chain': chain = args.chain; return null;
      case 'set_aws_credentials': aws = { configured: true, region: args.region }; return null;
      case 'clear_aws_credentials': throw 'Preview: the sample AWS key is not removed.';
      case 'list_bedrock_models': return { source: 'fallback', region: aws.region, reason: 'no_credentials', missing_permissions: [], default_model: { id: 'sample-bedrock', name: 'Sample Bedrock model' }, models: [] };
      case 'mail_save_settings': Object.assign(mailSettings, args.input); return null;
      case 'add_local_endpoint': { const ep = { ...endpoints[0], id: `sample-${endpoints.length + 1}`, name: args.name, url: args.url, backend_type: args.backendType ?? 'generic', last_health_status: false }; endpoints = [...endpoints, ep]; return ep; }
      case 'remove_local_endpoint': endpoints = endpoints.filter(e => e.id !== args.id); return null;
      case 'test_endpoint_connection': return { reachable: true, model_count: 1, latency_ms: 42 };
      case 'list_endpoint_models': return [{ id: 'sample-local', endpoint_id: args.endpointId, context_length: 8192, supports_tools: 'native' }];
      case 'check_system_compatibility': return { os: 'macos', arch: 'aarch64', total_ram_gb: 16, available_ram_gb: 9, gpu: { has_metal: true, has_cuda: false, cuda_version: null }, ollama: {}, vllm: {}, recommended_max_params: '8B' };
      case 'check_backend_prerequisites': return args.backendType === 'ollama' ? [{ name: 'Ollama installed', description: '', passed: true, required: true }, { name: 'Ollama running', description: '', passed: false, required: true, fix_command: 'ollama serve' }] : [{ name: 'Server reachable', description: '', passed: false, required: true, fix_label: 'Start the server, then check again.' }];
      case 'get_recommended_models': return [{ id: 'sample-local', name: 'Sample local model', param_size: '8B', download_size_gb: 5.2, min_ram_gb: 16, tool_capable: true, description: 'A sample model for the preview.', already_pulled: true }, { id: 'sample-local-small', name: 'Sample small model', param_size: '3B', download_size_gb: 2, min_ram_gb: 8, tool_capable: false, description: 'A smaller sample.', already_pulled: false }];
      case 'pull_model': throw 'Preview: models are not downloaded.';
      case 'cancel_model_pull': return null;
      case 'google_status': return true;
      case 'get_google_account': return 'alex@example.com';
      case 'get_aws_credentials_status': return { ...aws };
      case 'list_local_endpoints': return endpoints;
      case 'get_provider_chain': return chain;
      case 'list_tts_voices': return [];
      case 'get_wake_word_status': return { enabled: wakeOn, running: wakeOn, model_downloaded: true, voice_state: wakeOn ? 'WakeWordListening' : 'Idle' };
      case 'enable_wake_word': case 'disable_wake_word': wakeOn = command === 'enable_wake_word'; void emit('voice-state', wakeOn ? 'WakeWordListening' : 'Idle'); return null;
      case 'get_voice_settings': return null;
      case 'get_preference': return '';
      case 'is_model_downloaded': return false;
      case 'stop_speaking': return null;
      case 'google_accounts': return { main: { state: 'connected', name: 'Alex Chan', email: 'alex@example.com', gmail: true, calendar: true }, others: [] };
      case 'current_ai': return currentAiValue;
      // Nothing is sent or saved: the preview answers as if the test passed and keeps no key.
      case 'choose_ai': { const c = args.choice; const label = c.provider === 'compatible' ? 'Sample service' : c.provider === 'ollama' ? 'Ollama' : c.provider === 'gemini' ? 'Gemini' : c.provider === 'claude' ? 'Claude' : 'OpenAI'; currentAiValue = { provider: c.provider, label, model: c.model ?? null, base_url: c.baseUrl ?? null }; return currentAiValue; }
      case 'list_ai_models': return ['sample-chat', 'sample-chat-long'];
      case 'ai_consent_needed': case 'ai_google_content_note': return null;
      case 'probe_local_setup': return { os: 'macos', arch: 'aarch64', macosVersion: '15.2', totalRamGb: 16, availableDiskGb: 120, ollamaInstalled: false, ollamaRunning: false, recommendedTag: 'sample-local', downloadUrl: 'https://ollama.com/download/mac', installedModels: [] };
      case 'list_ai_consents': return consents;
      case 'revoke_ai_consent': consents = consents.filter(c => c.provider !== args.provider); void emit('ai-consent-changed'); return null;
      case 'notion_connection_status': case 'github_connection_status': case 'obsidian_connection_status': return integrations[command.split('_')[0]];
      case 'notion_disconnect': case 'github_disconnect': case 'obsidian_disconnect': integrations[command.split('_')[0]].status = 'not_connected'; return null;
      case 'get_storage_usage': return { ...storage };
      case 'set_retention_policy': storage.retentionDays = args.days; storage.retentionEnabled = args.enabled; return null;
      case 'run_storage_sweep': return { mailBodiesCleared: 0, conversationsDeleted: 0, emailsDeleted: 0, usageRowsRolledUp: 0, briefingsDeleted: 0 };
      case 'purge_storage': throw 'Preview: nothing is deleted from sample data.';
      case 'delete_all_data': throw 'Preview: nothing is deleted from sample data.';
      case 'ai_models': return catalog;
      default: throw new Error(`Preview: ${command} is not connected to a service.`);
    }
  }, { shouldMockEvents: true });
  document.title = 'JARVIS — workspace preview (sample data)';
  const banner = document.createElement('div'); banner.textContent = 'Preview · Sample data';
  banner.style.cssText = 'position:fixed;bottom:4px;left:14px;z-index:250;font:10px system-ui;color:#9bb1c7;pointer-events:none';
  document.body.append(banner);
}
