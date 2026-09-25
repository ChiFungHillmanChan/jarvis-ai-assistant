// Shared billing boundaries live alongside the existing Stripe/AWS dependencies.
import { randomUUID } from 'node:crypto';
import { SecretsManagerClient, GetSecretValueCommand } from '@aws-sdk/client-secrets-manager';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, GetCommand, UpdateCommand } from '@aws-sdk/lib-dynamodb';
import Stripe from 'stripe';

export type Tier = 'free' | 'pro' | 'team';
export type PaidTier = Exclude<Tier, 'free'>;
export const ddb = DynamoDBDocumentClient.from(new DynamoDBClient({}));
const sm = new SecretsManagerClient({});
let cached: { stripe: Stripe; webhookSecret: string } | undefined;
export const appDomain = process.env.APP_DOMAIN || 'jarvis-automation.com';

export class BillingError extends Error {
  constructor(public readonly code: string, public readonly statusCode = 500) { super(code); }
}
export function json(statusCode: number, value: object) {
  return { statusCode, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(value) };
}
export function failure(error: unknown) {
  if (error instanceof BillingError) return json(error.statusCode, { error: error.code });
  console.error('Billing operation failed');
  return json(500, { error: 'billing_unavailable' });
}
export function prices(): Record<PaidTier, string> {
  const sandbox = process.env.ENV === 'sandbox';
  const pro = process.env.STRIPE_PRO_PRICE_ID || (sandbox ? 'price_1TUsAuJCG4qg3rNcZVpOopcD' : '');
  const team = process.env.STRIPE_TEAM_PRICE_ID || (sandbox ? 'price_1TUsAwJCG4qg3rNcPY3YJiOB' : '');
  if (!pro.startsWith('price_') || !team.startsWith('price_') || pro === team) {
    throw new BillingError('billing_prices_not_configured', 503);
  }
  return { pro, team };
}
export async function loadBilling() {
  if (cached) return cached;
  const out = await sm.send(new GetSecretValueCommand({ SecretId: process.env.STRIPE_SECRET_NAME! }));
  const secret = JSON.parse(out.SecretString!);
  cached = { stripe: new Stripe(secret.apiKey, { apiVersion: '2025-02-24.acacia' }), webhookSecret: secret.webhookSecret };
  return cached;
}
export async function userRow(userId: string): Promise<Record<string, any>> {
  const out = await ddb.send(new GetCommand({ TableName: process.env.USER_META_TABLE!, Key: { userId }, ConsistentRead: true }));
  return out.Item || {};
}
export async function updateUser(userId: string, expression: string, values: Record<string, unknown>, owner: string) {
  await ddb.send(new UpdateCommand({
    TableName: process.env.USER_META_TABLE!, Key: { userId }, UpdateExpression: expression,
    ConditionExpression: 'billingLockOwner = :owner', ExpressionAttributeValues: { ...values, ':owner': owner },
  }));
}
// The lease exceeds the Lambda's timeout, so two live invocations cannot mutate
// the same account. A killed invocation leaves a short, recoverable lease.
export async function withBillingLock<T>(userId: string, work: (owner: string) => Promise<T>): Promise<T> {
  const owner = randomUUID();
  const now = Date.now();
  try {
    await ddb.send(new UpdateCommand({
      TableName: process.env.USER_META_TABLE!, Key: { userId },
      UpdateExpression: 'SET billingLockOwner = :owner, billingLockUntil = :until',
      ConditionExpression: 'attribute_not_exists(billingLockUntil) OR billingLockUntil < :now',
      ExpressionAttributeValues: { ':owner': owner, ':until': now + 120_000, ':now': now },
    }));
  } catch (error) {
    if ((error as Error).name === 'ConditionalCheckFailedException') throw new BillingError('billing_busy', 409);
    throw error;
  }
  try { return await work(owner); }
  finally {
    // A failed release must not hide the original result; the lease expires.
    try { await updateUser(userId, 'REMOVE billingLockOwner, billingLockUntil', {}, owner); }
    catch { console.warn('Billing lease release failed; awaiting expiry'); }
  }
}
export function objectId(value: string | { id: string } | null | undefined): string | null {
  return typeof value === 'string' ? value : value?.id || null;
}
export async function verifyCustomer(stripe: Stripe, customerId: string, userId: string) {
  const customer = await stripe.customers.retrieve(customerId);
  if (customer.deleted || customer.metadata.userId !== userId || customer.metadata.project !== 'jarvis') {
    throw new BillingError('billing_customer_mismatch');
  }
}
export function isLiveSubscriptionStatus(status: Stripe.Subscription.Status): boolean {
  return status !== 'canceled' && status !== 'incomplete_expired';
}
export async function subscriptionsFor(stripe: Stripe, customer: string) {
  const subscriptions: Stripe.Subscription[] = [];
  let after: string | undefined;
  do {
    const page = await stripe.subscriptions.list({ customer, status: 'all', limit: 100, ...(after ? { starting_after: after } : {}) });
    subscriptions.push(...page.data);
    after = page.has_more ? page.data.at(-1)?.id : undefined;
    if (page.has_more && !after) throw new BillingError('billing_subscription_page_invalid');
  } while (after);
  return subscriptions;
}
export async function portalSession(stripe: Stripe, customer: string, locale: 'en' | 'zh-HK' = 'en') {
  const configuration = process.env.STRIPE_PORTAL_CONFIGURATION_ID;
  if (!configuration && process.env.ENV !== 'sandbox') throw new BillingError('billing_portal_not_configured', 503);
  return stripe.billingPortal.sessions.create({ customer, locale, return_url: `https://${appDomain}/${locale}/`, ...(configuration ? { configuration } : {}) });
}
