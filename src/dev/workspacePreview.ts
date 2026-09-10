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
  const accounts = [{ id: 'sample-account', email: 'alex@example.com', connected: true, message_count: 4, last_synced: now.toISOString(), next_page_token: null }];
  const groups = [{ id: 1, name: 'Customer follow-ups', instructions: 'Summarize the decisions and follow-ups.', message_count: 2 }];
  const messages = ['September project review', 'Proposal: next steps', 'Team planning for Friday', 'Weekly product update'].map((subject, index) => ({ id: index + 1, account_id: 'sample-account', account_email: 'alex@example.com', provider_id: `sample-${index}`, thread_id: null, subject, sender: index % 2 ? 'Jordan Lee <jordan@example.com>' : 'Sam Taylor <sam@example.com>', snippet: 'Here are the details for our next review. Please check the timeline and proposed next steps.', body: `Hi Alex,\n\nHere are the details for our next review. The proposal is ready for your feedback.\n\nCould we review the timeline on Friday?\n\nThanks,\nSam`, is_read: index > 1, is_inbox: true, received_at: now.toISOString() }));
  const events = [{ id: 1, google_id: 'sample-event', summary: 'Customer onboarding review', description: 'Review the rollout plan and answer open questions.', location: 'Meeting room 2', start_time: day + 'T14:00:00Z', end_time: day + 'T14:45:00Z', attendees: '', status: 'confirmed' }];
  const github = [{ id: 1, title: 'Improve onboarding checklist', repo: 'example/workspace', number: 42, state: 'open', item_type: 'issue', url: 'https://github.com/example/workspace/issues/42', updated_at: now.toISOString() }];
  const notes = [{ id: 1, notion_id: '12345678901234567890123456789012ab', title: 'Customer onboarding playbook', url: 'https://www.notion.so/12345678901234567890123456789012ab', parent_type: 'workspace', last_edited: now.toISOString() }];
  const jobs = [{ id: 1, name: 'Morning briefing', schedule: '0 0 9 * * MON-FRI', action_type: 'briefing', status: 'active', last_run: day + ' 09:00:00', next_run: null, description: 'Review my calendar every weekday at 9 am.' }, { id: 2, name: 'Weekly project review', schedule: '0 0 16 * * FRI', action_type: 'summary', status: 'paused', last_run: null, next_run: null, description: 'Summarize project progress on Friday.' }];
  const runs = [{ id: 1, job_id: 1, started_at: day + ' 09:00:00', finished_at: day + ' 09:00:12', status: 'success', result: 'Your calendar has one customer review today.\n\nPrepare the rollout plan before the meeting.', error: null }];
  mockIPC(async (command, raw) => {
    const args = (raw || {}) as Record<string, any>;
    if (command.startsWith('plugin:')) return null;
    switch (command) {
      case 'check_onboarding_status': return true;
      case 'get_settings': return { values: { ai_provider: 'gemini_primary', google_email: 'alex@example.com', ...saved } };
      case 'update_setting': saved[args.key] = args.value; localStorage.setItem('jarvis-workspace-preview', JSON.stringify(saved)); return null;
      case 'get_conversations': return conversations[args.sessionId || 'legacy'] || [];
      case 'list_chat_sessions': return Object.entries(conversations).map(([id, entries]) => ({ id, title: entries[0]?.content.slice(0, 60) || 'Task', updated_at: now.toISOString() }));
      case 'send_message': {
        const { sessionId, runId } = args; const scope = { session_id: sessionId, run_id: runId };
        const user: ChatMessage = { id: null, role: 'user', content: args.message, created_at: now.toISOString() };
        conversations[sessionId] = [...(conversations[sessionId] || []), user];
        await emit('chat-state', { ...scope, state: 'thinking' });
        await emit('chat-status', { ...scope, phase: 'acting', status: 'Reading the sample calendar…' });
        await new Promise(resolve => setTimeout(resolve, 2500));
        await emit('chat-tool-result', { ...scope, tool_call_id: crypto.randomUUID(), tool_name: 'list_events', target: { view: 'calendar', accountId: 'legacy-google', dateRange: { from: day, to: day } }, result: { events } });
        const content = 'You have a customer onboarding review at 14:00 UTC. I opened the calendar result so you can review the details.';
        await emit('chat-token', { ...scope, token: content, done: false });
        await new Promise(resolve => setTimeout(resolve, 1200));
        await emit('chat-state', { ...scope, state: 'idle' });
        const answer: ChatMessage = { id: null, role: 'assistant', content, created_at: now.toISOString() };
        conversations[sessionId].push(answer); localStorage.setItem('jarvis-preview-conversations', JSON.stringify(conversations)); return answer;
      }
      case 'get_voice_state': return 'Idle';
      case 'get_wallpaper_status': case 'is_wallpaper_raised': return false;
      case 'get_tasks': case 'get_emails': case 'mail_list_memories': return [];
      case 'get_events': case 'get_todays_events': return events;
      case 'get_github_items': return github;
      case 'get_notion_pages': return notes;
      case 'get_cron_jobs': return jobs.map(j => ({ ...j }));
      case 'get_cron_runs': return args.jobId === 1 ? runs : [];
      case 'get_upcoming_runs': return [day + 'T09:00:00Z'];
      case 'preview_custom_cron': return { name: 'Calendar sync', schedule: '0 0 9 * * MON-FRI', action_type: 'sync_calendar', description: args.description };
      case 'create_reviewed_cron': { const j = { id: jobs.length + 1, ...args.interpretation, status: 'active', last_run: null, next_run: null }; jobs.push(j); return j; }
      case 'set_cron_job_status': { const j = jobs.find(v => v.id === args.jobId); if (!j) throw 'Sample routine not found'; j.status = args.status; return j.status; }
      case 'toggle_cron_job': { const j = jobs.find(v => v.id === args.id); if (!j) throw 'Sample routine not found'; j.status = j.status === 'active' ? 'paused' : 'active'; return j.status; }
      case 'mail_workspace': return { accounts, groups, settings: mailSettings };
      case 'mail_list_messages': { const filtered = messages.filter(m => (!args.query || m.subject.toLowerCase().includes(args.query.toLowerCase())) && (!args.groupId || m.id <= 2)); return { messages: filtered, total: filtered.length }; }
      case 'mail_get_message': return messages.find(m => m.id === args.messageId);
      case 'mail_analyze_group': return { text: 'Two customer conversations need follow-up.\n\n- Confirm the project review date.\n- Send feedback on the proposal.', mode: 'local', model: 'Preview only', source_ids: [1, 2], omitted_messages: 0 };
      case 'get_api_key_status': return { gemini: false, claude: false, openai: false };
      case 'google_status': return true;
      case 'get_google_account': return 'alex@example.com';
      case 'get_aws_credentials_status': return { configured: false, region: null };
      case 'list_local_endpoints': case 'get_provider_chain': case 'list_tts_voices': return [];
      case 'get_wake_word_status': return { enabled: false, model_downloaded: true };
      case 'get_voice_settings': return null;
      case 'get_preference': return '';
      case 'is_model_downloaded': return false;
      case 'stop_speaking': return null;
      default: throw new Error(`Preview: ${command} is not connected to a service.`);
    }
  }, { shouldMockEvents: true });
  document.title = 'JARVIS — workspace preview (sample data)';
  const banner = document.createElement('div'); banner.textContent = 'Preview · Sample data';
  banner.style.cssText = 'position:fixed;bottom:4px;left:14px;z-index:250;font:10px system-ui;color:#9bb1c7;pointer-events:none';
  document.body.append(banner);
}
