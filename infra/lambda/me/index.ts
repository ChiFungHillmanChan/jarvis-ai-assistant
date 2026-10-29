import type { APIGatewayProxyEventV2WithJWTAuthorizer, APIGatewayProxyResultV2 } from 'aws-lambda';
import { AdminGetUserCommand, CognitoIdentityProviderClient } from '@aws-sdk/client-cognito-identity-provider';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, GetCommand, QueryCommand, UpdateCommand } from '@aws-sdk/lib-dynamodb';
import { clientInfo, meBody } from './body';
import { readTrialDays } from '../stripe-checkout/trial';
import { effectiveConfig } from '../ai-proxy/config';
import { jarvisAiStatus, resolvePlan } from '../ai-proxy/entitlement';
import type { Grant } from '../ai-proxy/entitlement';

const cognito = new CognitoIdentityProviderClient({});
const ddb = DynamoDBDocumentClient.from(new DynamoDBClient({}));

async function emailFor(username: string): Promise<string | null> {
  const out = await cognito.send(new AdminGetUserCommand({
    UserPoolId: process.env.USER_POOL_ID!,
    Username: username,
  }));
  return out.UserAttributes?.find((a) => a.Name === 'email')?.Value ?? null;
}

/** Same read as the proxy's loadUser, so /me and the proxy agree on the grants. */
async function grantsFor(userId: string): Promise<Grant[]> {
  const items: Grant[] = [];
  let startKey: Record<string, unknown> | undefined;
  do {
    const page = await ddb.send(new QueryCommand({
      TableName: process.env.AI_GRANTS_TABLE!,
      KeyConditionExpression: 'userId = :u',
      ExpressionAttributeValues: { ':u': userId },
      ExclusiveStartKey: startKey,
    }));
    items.push(...((page.Items ?? []) as Grant[]));
    startKey = page.LastEvaluatedKey;
  } while (startKey);
  return items;
}

/** Record that the user was seen. The write may create the user-meta row; meBody treats a row with no tier as free. */
async function touch(sub: string, headers: Record<string, string | undefined>, now: Date): Promise<void> {
  const { appVersion, mailboxCount } = clientInfo(headers);
  const sets = ['lastSeenAt = :now'];
  const values: Record<string, unknown> = { ':now': now.toISOString() };
  if (appVersion !== null) { sets.push('appVersion = :v'); values[':v'] = appVersion; }
  if (mailboxCount !== null) { sets.push('mailboxCount = :c'); values[':c'] = mailboxCount; }
  try {
    await ddb.send(new UpdateCommand({
      TableName: process.env.USER_META_TABLE!,
      Key: { userId: sub },
      UpdateExpression: `SET ${sets.join(', ')}`,
      ExpressionAttributeValues: values,
    }));
  } catch {
    console.log(JSON.stringify({ event: 'me_touch_failed' }));
  }
}

export async function handler(event: APIGatewayProxyEventV2WithJWTAuthorizer): Promise<APIGatewayProxyResultV2> {
  const claims = event.requestContext.authorizer.jwt.claims;
  const sub = typeof claims.sub === 'string' ? claims.sub : '';
  const username = typeof claims.username === 'string' ? claims.username : '';
  if (!sub || !username) return { statusCode: 401, body: 'Missing identity' };

  const now = new Date();
  const [row, grants, configRow, email] = await Promise.all([
    ddb.send(new GetCommand({ TableName: process.env.USER_META_TABLE!, Key: { userId: sub } })),
    grantsFor(sub),
    ddb.send(new GetCommand({ TableName: process.env.AI_CONFIG_TABLE!, Key: { config: 'current' } })),
    emailFor(username),
    touch(sub, event.headers ?? {}, now),
  ]);
  const { config } = effectiveConfig(configRow.Item);
  // The period key comes only from resolvePlan, so /me and the proxy always count the same month.
  const plan = resolvePlan(row.Item, grants, config, now);
  const used = plan.periodKey
    ? ((await ddb.send(new GetCommand({ TableName: process.env.AI_PERIOD_TABLE!, Key: { userId: sub, periodKey: plan.periodKey } }))).Item?.billedMicros as number | undefined) ?? 0
    : 0;
  return {
    statusCode: 200,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      ...meBody(sub, email, row.Item, readTrialDays(process.env.TRIAL_DAYS) ?? 0),
      jarvisAi: jarvisAiStatus(plan, used, config.globalPause, row.Item),
    }),
  };
}
