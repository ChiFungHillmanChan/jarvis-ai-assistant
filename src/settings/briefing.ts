/** A briefing time as the backend takes it ("HH:MM", 00:00-23:59), or null. "8:05" reads as "08:05". */
export function normalizeBriefingTime(input: string): string | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(input.trim());
  if (!match) return null;
  const [hours, minutes] = [Number(match[1]), Number(match[2])];
  if (hours > 23 || minutes > 59) return null;
  return `${String(hours).padStart(2, '0')}:${match[2]}`;
}
