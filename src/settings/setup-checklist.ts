// The Setup guide's checklist: what a new user still has to do, in the order
// the setup guide walks it. Pure, so it is tested under node.

export interface ChecklistInput { aiReady: boolean; googleConnected: boolean; autoTriage: boolean; briefingTime: string | null }
export type ChecklistId = 'ai' | 'google' | 'triage' | 'briefing';

export function setupChecklist(i: ChecklistInput): { id: ChecklistId; done: boolean }[] {
  return [
    { id: 'ai', done: i.aiReady },
    { id: 'google', done: i.googleConnected },
    { id: 'triage', done: i.autoTriage },
    { id: 'briefing', done: (i.briefingTime ?? '').trim() !== '' },
  ];
}
