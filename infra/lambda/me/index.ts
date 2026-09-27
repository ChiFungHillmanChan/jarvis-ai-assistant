import type { APIGatewayProxyEventV2WithJWTAuthorizer, APIGatewayProxyResultV2 } from 'aws-lambda';
import { AdminGetUserCommand, CognitoIdentityProviderClient } from '@aws-sdk/client-cognito-identity-provider';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, GetCommand } from '@aws-sdk/lib-dynamodb';
import { meBody } from './body';
import { readTrialDays } from '../stripe-checkout/trial';

const cognito = new CognitoIdentityProviderClient({});
const ddb = DynamoDBDocumentClient.from(new DynamoDBClient({}));

async function emailFor(username: string): Promise<string | null> {
  const out = await cognito.send(new AdminGetUserCommand({
    UserPoolId: process.env.USER_POOL_ID!,
    Username: username,
  }));
  return out.UserAttributes?.find((a) => a.Name === 'email')?.Value ?? null;
}

export async function handler(event: APIGatewayProxyEventV2WithJWTAuthorizer): Promise<APIGatewayProxyResultV2> {
  const claims = event.requestContext.authorizer.jwt.claims;
  const sub = typeof claims.sub === 'string' ? claims.sub : '';
  const username = typeof claims.username === 'string' ? claims.username : '';
  if (!sub || !username) return { statusCode: 401, body: 'Missing identity' };

  const [row, email] = await Promise.all([
    ddb.send(new GetCommand({ TableName: process.env.USER_META_TABLE!, Key: { userId: sub } })),
    emailFor(username),
  ]);
  return {
    statusCode: 200,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(meBody(sub, email, row.Item, readTrialDays(process.env.TRIAL_DAYS) ?? 0)),
  };
}
