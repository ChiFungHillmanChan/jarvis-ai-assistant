export interface ToolResult {
  session_id: string;
  run_id: string;
  tool_call_id: string;
  tool_name: string;
  target: { view: 'email' | 'calendar' | 'github' | 'notion'; [key: string]: unknown };
  result: Record<string, unknown>;
}
const toolsByView: Record<string, string[]> = {
  email: ['read_email', 'draft_email'], calendar: ['list_events', 'create_event', 'update_event'],
  github: ['list_github_items', 'create_github_issue'], notion: ['read_notion_page', 'create_notion_page'],
};
export function validResult(value: unknown): value is ToolResult {
  if (!value || typeof value !== 'object') return false;
  const p = value as ToolResult;
  if (![p.session_id, p.run_id, p.tool_call_id, p.tool_name].every(v => typeof v === 'string' && v.length > 0)) return false;
  if (!p.target || !p.result || typeof p.result !== 'object' || Array.isArray(p.result)) return false;
  if (!toolsByView[p.target.view]?.includes(p.tool_name)) return false;
  if (p.target.view === 'email') return p.target.store === 'legacy' && p.target.accountId === 'legacy-google' && typeof (p.target.messageId || p.target.draftId) === 'string';
  if (p.target.view === 'calendar') return p.target.accountId === 'legacy-google';
  if (p.target.view === 'notion') return p.target.provider === 'notion' && typeof p.target.pageId === 'string';
  return p.target.repo == null || typeof p.target.repo === 'string';
}
export function resultTitle(p: ToolResult): string {
  const names: Record<string, string> = { read_email: 'Email', draft_email: 'Email draft', list_events: 'Calendar results', create_event: 'Calendar event', update_event: 'Updated event', list_github_items: 'GitHub results', create_github_issue: 'GitHub issue', read_notion_page: 'Note', create_notion_page: 'New note' };
  return typeof p.result.subject === 'string' ? p.result.subject.slice(0, 64) : typeof p.result.title === 'string' ? p.result.title.slice(0, 64) : names[p.tool_name] || 'Tool result';
}
// Same provider object reopens its existing workspace, even from another task.
export function resultIdentity(p: ToolResult): string {
  const t = p.target;
  if (t.view === 'email') return JSON.stringify(['email', t.store, t.accountId, t.messageId || null, t.draftId || null]);
  if (t.view === 'calendar') return JSON.stringify(['calendar', t.accountId, t.eventId || null, t.eventId ? null : t.dateRange]);
  if (t.view === 'notion') return JSON.stringify(['notion', t.pageId]);
  return JSON.stringify(['github', t.repo || null, p.result.providerUrl || null]);
}
export function providerUrl(p: ToolResult): string | null {
  if (p.target.view === 'email') return 'https://mail.google.com/mail/u/0/' + (p.target.draftId ? '#drafts' : '#all/' + encodeURIComponent(String(p.target.messageId)));
  if (p.target.view === 'calendar') return 'https://calendar.google.com/calendar/u/0/r';
  if (p.target.view === 'notion' && /^[a-f\d-]{32,36}$/i.test(String(p.target.pageId))) return 'https://www.notion.so/' + String(p.target.pageId).replace(/-/g, '');
  if (p.target.view === 'github' && typeof p.result.providerUrl === 'string') {
    try { const u = new URL(p.result.providerUrl); if (u.protocol === 'https:' && u.hostname === 'github.com' && !u.username && !u.password) return u.href; } catch { /* Invalid provider references are not navigable. */ }
  }
  return null;
}
