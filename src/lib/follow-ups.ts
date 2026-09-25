import type { Task } from './types';

/** YYYY-MM-DD for the local calendar day, the format tasks.deadline is stored in. */
export function localDate(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

/**
 * Open tasks due within three local days, overdue first. The same window the
 * scheduler's deadline monitor uses (DEADLINE_WINDOW_QUERY in scheduler/jobs.rs).
 */
export function followUps(tasks: Task[], today: string): Task[] {
  const [y, m, d] = today.split('-').map(Number);
  const limit = localDate(new Date(y, m - 1, d + 3));
  return tasks
    .filter(t => t.status !== 'completed' && t.deadline !== null && t.deadline <= limit)
    .sort((a, b) => a.deadline!.localeCompare(b.deadline!) || (a.id ?? 0) - (b.id ?? 0));
}
