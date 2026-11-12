import type { APIGatewayProxyEventV2WithJWTAuthorizer, APIGatewayProxyResultV2 } from 'aws-lambda';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, GetCommand, UpdateCommand } from '@aws-sdk/lib-dynamodb';
import { accessCutoff, accessRequest } from './access';

const ddb = DynamoDBDocumentClient.from(new DynamoDBClient({}));

const reply = (requestedAt: string): APIGatewayProxyResultV2 => ({
  statusCode: 200,
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ requestedAt }),
});

export async function handler(event: APIGatewayProxyEventV2WithJWTAuthorizer): Promise<APIGatewayProxyResultV2> {
  const claims = event.requestContext.authorizer.jwt.claims;
  const sub = typeof claims.sub === 'string' ? claims.sub : '';
  if (!sub) return { statusCode: 401, body: 'Missing identity' };

  const table = process.env.USER_META_TABLE!;
  const now = new Date();
  const { requestedAt } = accessRequest(undefined, now);
  try {
    // The condition makes this atomic: a second request inside 24 h cannot overwrite the first.
    await ddb.send(new UpdateCommand({
      TableName: table,
      Key: { userId: sub },
      UpdateExpression: 'SET aiAccessRequestedAt = :now REMOVE aiAccessRequestDismissedAt',
      ConditionExpression: 'attribute_not_exists(aiAccessRequestedAt) OR aiAccessRequestedAt < :cutoff',
      ExpressionAttributeValues: { ':now': requestedAt, ':cutoff': accessCutoff(now) },
    }));
    return reply(requestedAt);
  } catch (error) {
    if ((error as { name?: unknown } | null)?.name !== 'ConditionalCheckFailedException') throw error;
  }
  const row = await ddb.send(new GetCommand({ TableName: table, Key: { userId: sub }, ConsistentRead: true }));
  const stored = row.Item?.aiAccessRequestedAt;
  if (typeof stored !== 'string') throw new Error('access request not recorded');
  return reply(stored);
}
