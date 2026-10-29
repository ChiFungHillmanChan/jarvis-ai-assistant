import { CATALOG } from './config';
import type { AiConfig, CatalogEntry, Workload } from './config';
import { denial, resolvePlan } from './entitlement';
import type { Grant } from './entitlement';
import { userHash } from './log';
import type { RequestLog } from './log';
import { billedMicros, costMicros, estimateUsage } from './pricing';
import type { Usage } from './pricing';
import { parseRequest } from './request';
import { relay } from './stream';
import type { ProxyEvent } from './stream';

export type ErrorCode =
  | 'unauthenticated' | 'bad_request' | 'not_entitled' | 'cap_reached' | 'trial_credit_used' | 'suspended'
  | 'duplicate_request' | 'rate_limited' | 'paused' | 'model_unavailable' | 'upstream_error';

export const ERROR_STATUS: Record<ErrorCode, number> = {
  unauthenticated: 401, bad_request: 400, not_entitled: 402, cap_reached: 402, trial_credit_used: 402, suspended: 403,
  duplicate_request: 409, rate_limited: 429, paused: 503, model_unavailable: 502, upstream_error: 502,
};

export const RATE_PER_MINUTE: Record<Workload, number> = { chat: 30, mail_analysis: 30, mail_sort: 150, voice: 30 };
export const MAX_IN_FLIGHT = 8;
export const DAILY_ALERT_MICROS = 20_000_000;

export interface Sink {
  fail(status: number, body: Record<string, unknown>): void;
  start(): void;
  send(event: ProxyEvent): void;
  end(): void;
}

export interface Limits {
  hit(userId: string, workload: Workload, perMinute: number, now: Date): Promise<boolean>;
  acquire(userId: string, now: Date): Promise<boolean>;
  release(userId: string): Promise<void>;
  claim(userId: string, requestId: string, now: Date): Promise<boolean>;
  /** Returns the new daily total. */
  addDaily(userId: string, billedMicros: number, now: Date): Promise<number>;
  /** Billed total across all users for the Hong Kong day of `now`, 0 when nothing is counted yet. */
  globalDaily(now: Date): Promise<number>;
  /** Returns the new global daily total. */
  addGlobalDaily(billedMicros: number, now: Date): Promise<number>;
}

export interface UsageRecord {
  userId: string;
  requestId: string;
  ts: string;
  periodKey: string;
  workload: Workload;
  modelId: string;
  region: string;
  inputTokens: number;
  outputTokens: number;
  costMicros: number;
  billedMicros: number;
  status: 'ok' | 'error';
  latencyMs: number;
  appVersion: string | null;
}

export interface BedrockInput {
  modelId: string;
  messages: unknown[];
  system?: { text: string }[];
  toolConfig?: Record<string, unknown>;
  inferenceConfig: { maxTokens: number };
}

export interface Deps {
  now(): Date;
  loadUser(userId: string): Promise<{ meta: Record<string, unknown> | undefined; grants: Grant[] }>;
  loadConfig(): Promise<AiConfig>;
  periodUsed(userId: string, periodKey: string): Promise<number>;
  limits: Limits;
  openStream(entry: CatalogEntry, input: BedrockInput): Promise<AsyncIterable<Record<string, any>>>;
  meter(record: UsageRecord): Promise<void>;
  log(entry: RequestLog): void;
  dailyAlarm(userHash: string): void;
  globalCeilingAlarm(): void;
}

/** Only the exception name is read, never its message: provider errors can echo prompt content. */
function codeForOpenError(error: unknown): ErrorCode {
  const name = typeof error === 'object' && error !== null ? (error as { name?: unknown }).name : undefined;
  if (name === 'ThrottlingException' || name === 'ServiceQuotaExceededException') return 'rate_limited';
  if (name === 'AccessDeniedException' || name === 'ResourceNotFoundException' || name === 'ModelNotReadyException' || name === 'ServiceUnavailableException') return 'model_unavailable';
  return 'upstream_error';
}

export async function converse(deps: Deps, caller: { userId: string; appVersion: string | null }, raw: string, sink: Sink): Promise<void> {
  const startedAt = deps.now().getTime();
  const entry: RequestLog = {
    userHash: userHash(caller.userId), workload: null, modelId: null, status: 'ok', code: null,
    inputTokens: 0, outputTokens: 0, billedMicros: 0, latencyMs: 0, meterFailed: false,
  };
  let streaming = false;

  // The client may already be gone; a failing sink must never change what is metered.
  const start = () => { try { sink.start(); } catch { /* disconnected */ } };
  const send = (event: ProxyEvent) => { try { sink.send(event); } catch { /* disconnected */ } };
  const end = () => { try { sink.end(); } catch { /* disconnected */ } };
  const refuse = (code: ErrorCode, extra: Record<string, unknown> = {}, status: RequestLog['status'] = 'denied') => {
    entry.status = status;
    entry.code = code;
    sink.fail(ERROR_STATUS[code], { error: code, ...extra });
  };

  try {
    const request = parseRequest(raw);
    if (!request) return refuse('bad_request', {}, 'error');
    entry.workload = request.workload;

    const [{ meta, grants }, config] = await Promise.all([deps.loadUser(caller.userId), deps.loadConfig()]);
    const now = deps.now();
    const plan = resolvePlan(meta, grants, config, now);
    const used = plan.periodKey ? await deps.periodUsed(caller.userId, plan.periodKey) : 0;
    const denied = denial(plan, used, config.globalPause);
    if (denied) {
      return refuse(denied, denied === 'cap_reached' ? { capMicros: plan.capMicros, periodEndsAt: plan.periodEndsAt } : {});
    }

    const { limits } = deps;
    // The Budgets kill switch acts on billing data that can lag a day; this ceiling is real time and fails closed.
    let globalTotal: number;
    try { globalTotal = await limits.globalDaily(now); } catch { return refuse('paused', {}, 'error'); }
    if (globalTotal >= config.globalDailyCapMicros) return refuse('paused');
    if (!(await limits.hit(caller.userId, request.workload, RATE_PER_MINUTE[request.workload], now))) return refuse('rate_limited');
    if (!(await limits.acquire(caller.userId, now))) return refuse('rate_limited');

    try {
      if (!(await limits.claim(caller.userId, request.requestId, now))) return refuse('duplicate_request');

      const route = config.models[request.workload];
      const input = (modelId: string): BedrockInput => ({
        modelId,
        messages: request.messages,
        ...(request.system ? { system: [{ text: request.system }] } : {}),
        ...(request.tools ? { toolConfig: request.tools } : {}),
        inferenceConfig: { maxTokens: request.maxTokens },
      });

      let modelId = route.primary;
      let events: AsyncIterable<Record<string, any>>;
      try {
        events = await deps.openStream(CATALOG[modelId], input(modelId));
      } catch (primaryError) {
        if (route.fallback === null) return refuse(codeForOpenError(primaryError), {}, 'error');
        modelId = route.fallback;
        try {
          events = await deps.openStream(CATALOG[modelId], input(modelId));
        } catch (fallbackError) {
          return refuse(codeForOpenError(fallbackError), {}, 'error');
        }
      }
      entry.modelId = modelId;
      const catalog = CATALOG[modelId];

      start();
      streaming = true;
      const summary = await relay(events, { dsml: catalog.dsml, hasTools: request.tools !== null }, send);

      const tokens: Usage | null = summary.usage ?? (summary.streamedChars > 0 ? estimateUsage(request.requestChars, summary.streamedChars) : null);
      let billed = 0;
      if (tokens) {
        const cost = costMicros(tokens, config.prices[modelId]);
        billed = billedMicros(cost, config.multiplierBps);
        const record: UsageRecord = {
          userId: caller.userId, requestId: request.requestId, ts: now.toISOString(), periodKey: plan.periodKey!,
          workload: request.workload, modelId, region: catalog.region,
          inputTokens: tokens.inputTokens, outputTokens: tokens.outputTokens, costMicros: cost, billedMicros: billed,
          status: summary.failed ? 'error' : 'ok', latencyMs: deps.now().getTime() - startedAt, appVersion: caller.appVersion,
        };
        try {
          await deps.meter(record);
        } catch {
          try { await deps.meter(record); } catch { entry.meterFailed = true; }
        }
        // The alarm is best effort: a failing counter must not fail a request that was already served.
        try {
          const dailyTotal = await limits.addDaily(caller.userId, billed, now);
          if (dailyTotal >= DAILY_ALERT_MICROS && dailyTotal - billed < DAILY_ALERT_MICROS) deps.dailyAlarm(entry.userHash);
        } catch { /* best effort */ }
        try {
          const total = await limits.addGlobalDaily(billed, now);
          const cap = config.globalDailyCapMicros;
          if (total >= cap && total - billed < cap) deps.globalCeilingAlarm();
        } catch { /* best effort */ }
        entry.inputTokens = tokens.inputTokens;
        entry.outputTokens = tokens.outputTokens;
        entry.billedMicros = billed;
      }

      if (summary.failed) {
        entry.status = 'error';
        entry.code = 'upstream_error';
        send({ type: 'error', code: 'upstream_error' });
      } else {
        send({
          type: 'done', stopReason: summary.stopReason ?? 'end_turn', modelId,
          inputTokens: entry.inputTokens, outputTokens: entry.outputTokens, billedMicros: billed,
          period: { key: plan.periodKey!, usedMicros: used + billed, capMicros: plan.capMicros },
        });
      }
      end();
    } finally {
      try { await limits.release(caller.userId); } catch { /* the slot expires on its own */ }
    }
  } catch {
    entry.status = 'error';
    entry.code = 'upstream_error';
    if (streaming) { send({ type: 'error', code: 'upstream_error' }); end(); }
    else sink.fail(ERROR_STATUS.upstream_error, { error: 'upstream_error' });
  } finally {
    entry.latencyMs = deps.now().getTime() - startedAt;
    deps.log(entry);
  }
}
