import { createHash } from 'node:crypto';
import type { Workload } from './config';

/** The only shape ever logged for a request. No prompt, reply, tool data or error text can appear in it. */
export interface RequestLog {
  userHash: string;
  workload: Workload | null;
  modelId: string | null;
  status: 'ok' | 'error' | 'denied';
  code: string | null;
  inputTokens: number;
  outputTokens: number;
  billedMicros: number;
  latencyMs: number;
  meterFailed: boolean;
}

export function userHash(userId: string): string {
  return createHash('sha256').update(userId).digest('hex').slice(0, 16);
}

export function writeLog(entry: RequestLog): void {
  console.log(JSON.stringify({ event: 'converse', ...entry }));
}

/** CloudWatch Embedded Metric Format line for the per-user daily billed alarm. */
export function writeDailyAlarmMetric(userHash: string, env: string): void {
  console.log(JSON.stringify({
    _aws: {
      Timestamp: Date.now(),
      CloudWatchMetrics: [{ Namespace: 'Jarvis/AiProxy', Dimensions: [['Env']], Metrics: [{ Name: 'UserDailyBilledOver20', Unit: 'Count' }] }],
    },
    Env: env,
    UserDailyBilledOver20: 1,
    userHash,
  }));
}
