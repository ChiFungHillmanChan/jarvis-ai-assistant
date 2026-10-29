import { BedrockRuntimeClient, ConverseStreamCommand } from '@aws-sdk/client-bedrock-runtime';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, GetCommand, PutCommand, QueryCommand, TransactWriteCommand, UpdateCommand } from '@aws-sdk/lib-dynamodb';
import { effectiveConfig } from './config';
import type { AiConfig, CatalogEntry, Workload } from './config';
import { MAX_IN_FLIGHT } from './converse';
import type { BedrockInput, Deps, UsageRecord } from './converse';
import type { Grant } from './entitlement';
import { writeDailyAlarmMetric, writeLog } from './log';

/** A leased in-flight slot outlives the 300 s Lambda timeout, so an expired lease can never still be running. */
export const LEASE_SECONDS = 360;
const USAGE_TTL_DAYS = 400;
const HK_OFFSET_MS = 8 * 3_600_000;

type Send = (cmd: any) => Promise<any>;

const isConditionFailed = (error: unknown): boolean =>
  typeof error === 'object' && error !== null && (error as { name?: unknown }).name === 'ConditionalCheckFailedException';

const epochSeconds = (now: Date): number => Math.floor(now.getTime() / 1000);

export async function acquireLease(send: Send, table: string, userId: string, now: Date): Promise<boolean> {
  const key = { pk: `inflight#${userId}` };
  const seconds = epochSeconds(now);
  const exp = seconds + LEASE_SECONDS;
  const take = async (): Promise<boolean> => {
    try {
      await send(new UpdateCommand({
        TableName: table,
        Key: key,
        UpdateExpression: 'ADD n :one SET expiresAt = :exp',
        ConditionExpression: 'attribute_not_exists(n) OR (n < :max AND expiresAt >= :now)',
        ExpressionAttributeValues: { ':one': 1, ':max': MAX_IN_FLIGHT, ':now': seconds, ':exp': exp },
      }));
      return true;
    } catch (error) {
      if (isConditionFailed(error)) return false;
      throw error;
    }
  };
  if (await take()) return true;
  // Full or expired. With no acquire in the last LEASE_SECONDS nothing can still be running, so reset the count.
  try {
    await send(new UpdateCommand({
      TableName: table,
      Key: key,
      UpdateExpression: 'SET n = :one, expiresAt = :exp',
      ConditionExpression: 'expiresAt < :now',
      ExpressionAttributeValues: { ':one': 1, ':now': seconds, ':exp': exp },
    }));
    return true;
  } catch (error) {
    if (!isConditionFailed(error)) throw error;
  }
  // A parallel request may have just won the reset and refreshed the lease, so try the normal acquire once more.
  return take();
}

export function cachedConfig(
  read: () => Promise<Record<string, unknown> | undefined>,
  clock: () => number,
  ttlMs = 30_000,
): () => Promise<AiConfig> {
  let value: AiConfig | null = null;
  let readAt = 0;
  return async () => {
    const t = clock();
    if (value && t - readAt <= ttlMs) return value;
    value = effectiveConfig(await read()).config;
    readAt = t;
    return value;
  };
}

/** Metadata only: no prompt, reply or tool data ever reaches the table. The app version is a short client label, not content. */
export function usageItem(record: UsageRecord): Record<string, unknown> {
  return {
    userId: record.userId,
    sk: `${record.ts}#${record.requestId}`,
    requestId: record.requestId,
    ts: record.ts,
    periodKey: record.periodKey,
    workload: record.workload,
    modelId: record.modelId,
    region: record.region,
    inputTokens: record.inputTokens,
    outputTokens: record.outputTokens,
    costMicros: record.costMicros,
    billedMicros: record.billedMicros,
    status: record.status,
    latencyMs: record.latencyMs,
    appVersion: record.appVersion,
    expiresAt: Math.floor(Date.parse(record.ts) / 1000) + USAGE_TTL_DAYS * 86_400,
  };
}

const hkDay = (now: Date): string => new Date(now.getTime() + HK_OFFSET_MS).toISOString().slice(0, 10);

export function createDeps(env: Record<string, string | undefined>): Deps {
  const ddb = DynamoDBDocumentClient.from(new DynamoDBClient({}));
  const send: Send = cmd => ddb.send(cmd);
  const limitsTable = env.AI_LIMITS_TABLE!;
  const usageTable = env.AI_USAGE_TABLE!;
  const periodTable = env.AI_PERIOD_TABLE!;
  const grantsTable = env.AI_GRANTS_TABLE!;
  const configTable = env.AI_CONFIG_TABLE!;
  const userMetaTable = env.USER_META_TABLE!;
  const alarmEnv = env.ENV ?? 'unknown';

  const bedrock = new Map<string, BedrockRuntimeClient>();
  let configWarned = false;
  const config = cachedConfig(async () => {
    const out = await ddb.send(new GetCommand({ TableName: configTable, Key: { config: 'current' } }));
    if (effectiveConfig(out.Item).invalid && !configWarned) {
      configWarned = true;
      console.log(JSON.stringify({ event: 'config_invalid' }));
    }
    return out.Item;
  }, Date.now);

  return {
    now: () => new Date(),
    async loadUser(userId) {
      const [meta, grants] = await Promise.all([
        ddb.send(new GetCommand({ TableName: userMetaTable, Key: { userId } })),
        (async () => {
          const items: Grant[] = [];
          let startKey: Record<string, unknown> | undefined;
          do {
            const page = await ddb.send(new QueryCommand({
              TableName: grantsTable,
              KeyConditionExpression: 'userId = :u',
              ExpressionAttributeValues: { ':u': userId },
              ExclusiveStartKey: startKey,
            }));
            items.push(...((page.Items ?? []) as Grant[]));
            startKey = page.LastEvaluatedKey;
          } while (startKey);
          return items;
        })(),
      ]);
      return { meta: meta.Item, grants };
    },
    loadConfig: config,
    async periodUsed(userId, periodKey) {
      const out = await ddb.send(new GetCommand({ TableName: periodTable, Key: { userId, periodKey } }));
      return (out.Item?.billedMicros as number | undefined) ?? 0;
    },
    limits: {
      async hit(userId, workload: Workload, perMinute, now) {
        try {
          await send(new UpdateCommand({
            TableName: limitsTable,
            Key: { pk: `rate#${userId}#${workload}#${Math.floor(now.getTime() / 60_000)}` },
            UpdateExpression: 'ADD n :one SET expiresAt = :exp',
            ConditionExpression: 'attribute_not_exists(n) OR n < :limit',
            ExpressionAttributeValues: { ':one': 1, ':limit': perMinute, ':exp': epochSeconds(now) + 120 },
          }));
          return true;
        } catch (error) {
          if (isConditionFailed(error)) return false;
          throw error;
        }
      },
      acquire: (userId, now) => acquireLease(send, limitsTable, userId, now),
      async release(userId) {
        try {
          await send(new UpdateCommand({
            TableName: limitsTable,
            Key: { pk: `inflight#${userId}` },
            UpdateExpression: 'ADD n :minus',
            ConditionExpression: 'n > :zero',
            ExpressionAttributeValues: { ':minus': -1, ':zero': 0 },
          }));
        } catch (error) {
          if (!isConditionFailed(error)) throw error;
        }
      },
      async claim(userId, requestId, now) {
        const seconds = epochSeconds(now);
        try {
          await send(new PutCommand({
            TableName: limitsTable,
            Item: { pk: `req#${userId}#${requestId}`, expiresAt: seconds + 86_400 },
            ConditionExpression: 'attribute_not_exists(pk) OR expiresAt < :now',
            ExpressionAttributeValues: { ':now': seconds },
          }));
          return true;
        } catch (error) {
          if (isConditionFailed(error)) return false;
          throw error;
        }
      },
      async addDaily(userId, billedMicros, now) {
        const out = await send(new UpdateCommand({
          TableName: limitsTable,
          Key: { pk: `day#${userId}#${hkDay(now)}` },
          UpdateExpression: 'ADD billedMicros :b SET expiresAt = :exp',
          ExpressionAttributeValues: { ':b': billedMicros, ':exp': epochSeconds(now) + 2 * 86_400 },
          ReturnValues: 'UPDATED_NEW',
        }));
        return out.Attributes.billedMicros as number;
      },
    },
    async openStream(entry: CatalogEntry, input: BedrockInput) {
      let client = bedrock.get(entry.region);
      if (!client) {
        client = new BedrockRuntimeClient({ region: entry.region });
        bedrock.set(entry.region, client);
      }
      const out = await client.send(new ConverseStreamCommand(input as any));
      return out.stream as AsyncIterable<Record<string, any>>;
    },
    async meter(record) {
      // The Put is conditional so converse's one retry with the same record cannot count the period twice.
      await send(new TransactWriteCommand({
        TransactItems: [
          { Put: { TableName: usageTable, Item: usageItem(record), ConditionExpression: 'attribute_not_exists(sk)' } },
          {
            Update: {
              TableName: periodTable,
              Key: { userId: record.userId, periodKey: record.periodKey },
              UpdateExpression: 'ADD billedMicros :b, costMicros :c, requests :one, #wb :b, #wr :one SET updatedAt = :ts',
              ExpressionAttributeNames: { '#wb': `billed_${record.workload}`, '#wr': `requests_${record.workload}` },
              ExpressionAttributeValues: { ':b': record.billedMicros, ':c': record.costMicros, ':one': 1, ':ts': record.ts },
            },
          },
        ],
      }));
    },
    log: writeLog,
    dailyAlarm: userHash => writeDailyAlarmMetric(userHash, alarmEnv),
  };
}
