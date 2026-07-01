import type { APIGatewayProxyEventV2, APIGatewayProxyResultV2 } from 'aws-lambda';
import { SecretsManagerClient, GetSecretValueCommand } from '@aws-sdk/client-secrets-manager';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, GetCommand, UpdateCommand } from '@aws-sdk/lib-dynamodb';
import Stripe from 'stripe';

const sm = new SecretsManagerClient({});
const ddb = DynamoDBDocumentClient.from(new DynamoDBClient({}));

let cachedSecrets: { apiKey: string } | null = null;
let cachedStripe: Stripe | null = null;

const PRICE_LOOKUP: Record<'pro' | 'team', string> = {
  pro: 'price_1TUsAuJCG4qg3rNcZVpOopcD',
  team: 'price_1TUsAwJCG4qg3rNcPY3YJiOB',
};

const APP_DOMAIN = process.env.APP_DOMAIN || 'jarvis-automation.com';

async function loadStripe() {
  if (cachedStripe) return cachedStripe;
  const out = await sm.send(new GetSecretValueCommand({ SecretId: process.env.STRIPE_SECRET_NAME! }));
  cachedSecrets = JSON.parse(out.SecretString!);
  cachedStripe = new Stripe(cachedSecrets!.apiKey, { apiVersion: '2024-12-18.acacia' });
  return cachedStripe;
}

async function getOrCreateCustomer(stripe: Stripe, userId: string, email: string): Promise<string> {
  const existing = await ddb.send(new GetCommand({
    TableName: process.env.USER_META_TABLE!,
    Key: { userId },
    ProjectionExpression: 'stripeCustomerId',
  }));
  const cached = existing.Item?.stripeCustomerId as string | undefined;
  if (cached) return cached;

  const customer = await stripe.customers.create({
    email,
    metadata: { userId, project: 'jarvis' },
  });
  await ddb.send(new UpdateCommand({
    TableName: process.env.USER_META_TABLE!,
    Key: { userId },
    UpdateExpression: 'SET stripeCustomerId = :c, email = :e, updatedAt = :u',
    ExpressionAttributeValues: {
      ':c': customer.id,
      ':e': email,
      ':u': new Date().toISOString(),
    },
  }));
  return customer.id;
}

interface CheckoutBody {
  tier: 'pro' | 'team';
  user_id: string;
  email: string;
}

export async function handler(event: APIGatewayProxyEventV2): Promise<APIGatewayProxyResultV2> {
  if (!event.body) return { statusCode: 400, body: 'Missing body' };

  let body: CheckoutBody;
  try {
    body = JSON.parse(event.isBase64Encoded ? Buffer.from(event.body, 'base64').toString('utf8') : event.body);
  } catch {
    return { statusCode: 400, body: 'Invalid JSON' };
  }

  if (!['pro', 'team'].includes(body.tier)) {
    return { statusCode: 400, body: `Invalid tier: ${body.tier}` };
  }
  if (!body.user_id || !body.email) {
    return { statusCode: 400, body: 'Missing user_id or email' };
  }

  const stripe = await loadStripe();
  const priceId = PRICE_LOOKUP[body.tier];

  try {
    const customerId = await getOrCreateCustomer(stripe, body.user_id, body.email);

    const session = await stripe.checkout.sessions.create({
      mode: 'subscription',
      customer: customerId,
      line_items: [{ price: priceId, quantity: 1 }],
      client_reference_id: body.user_id,
      subscription_data: {
        metadata: { userId: body.user_id, tier: body.tier, project: 'jarvis' },
      },
      success_url: `https://${APP_DOMAIN}/payment/success?session_id={CHECKOUT_SESSION_ID}`,
      cancel_url: `https://${APP_DOMAIN}/payment/cancel`,
      automatic_tax: { enabled: false },
      allow_promotion_codes: true,
    });

    return {
      statusCode: 200,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ url: session.url, session_id: session.id }),
    };
  } catch (err) {
    console.error('Failed to create checkout session', err);
    return { statusCode: 500, body: `Checkout creation failed: ${err instanceof Error ? err.message : 'unknown'}` };
  }
}
