import type { APIGatewayProxyEventV2WithJWTAuthorizer, APIGatewayProxyResultV2 } from 'aws-lambda';
import { AdminGetUserCommand, CognitoIdentityProviderClient } from '@aws-sdk/client-cognito-identity-provider';
import { SecretsManagerClient, GetSecretValueCommand } from '@aws-sdk/client-secrets-manager';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, GetCommand, UpdateCommand } from '@aws-sdk/lib-dynamodb';
import Stripe from 'stripe';
import { readTier } from './body';

const sm = new SecretsManagerClient({});
const ddb = DynamoDBDocumentClient.from(new DynamoDBClient({}));
const cognito = new CognitoIdentityProviderClient({});

async function emailFor(username: string): Promise<string | null> {
  const out = await cognito.send(new AdminGetUserCommand({
    UserPoolId: process.env.USER_POOL_ID!,
    Username: username,
  }));
  return out.UserAttributes?.find((a) => a.Name === 'email')?.Value ?? null;
}

let cachedSecrets: { apiKey: string } | null = null;
let cachedStripe: Stripe | null = null;

const PRICE_LOOKUP: Record<'pro' | 'team', string> = {
  pro: 'price_1TUsAuJCG4qg3rNcZVpOopcD',
  team: 'price_1TUsAwJCG4qg3rNcPY3YJiOB',
};

const APP_DOMAIN = process.env.APP_DOMAIN || 'jarvis-automation.com';

// Every subscription status in which Stripe still holds a subscription that can
// bill this customer. Only `canceled` and `incomplete_expired` are terminal.
//
// Filtering on 'active' alone was the hole: a card that fails at renewal moves
// the subscription to `past_due` and dunning runs for weeks. An 'active'-only
// guard sees nothing, mints a second Checkout Session, and when the first
// subscription recovers the customer has two live subscriptions -- the exact
// double-billing this guard exists to prevent. `unpaid`, `paused`, `trialing`
// and `incomplete` have the same shape.
//
// `incomplete` is deliberately included even though it self-expires after 23h:
// a subscription awaiting 3DS confirmation is still a subscription, and the
// billing portal's invoice history is where an unpaid first invoice gets paid.
const LIVE_SUBSCRIPTION_STATUSES: ReadonlySet<Stripe.Subscription.Status> = new Set([
  'active',
  'trialing',
  'past_due',
  'unpaid',
  'incomplete',
  'paused',
]);

export function isLiveSubscriptionStatus(status: Stripe.Subscription.Status): boolean {
  return LIVE_SUBSCRIPTION_STATUSES.has(status);
}

async function loadStripe() {
  if (cachedStripe) return cachedStripe;
  const out = await sm.send(new GetSecretValueCommand({ SecretId: process.env.STRIPE_SECRET_NAME! }));
  cachedSecrets = JSON.parse(out.SecretString!);
  cachedStripe = new Stripe(cachedSecrets!.apiKey, { apiVersion: '2025-02-24.acacia' });
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

export async function handler(event: APIGatewayProxyEventV2WithJWTAuthorizer): Promise<APIGatewayProxyResultV2> {
  // Identity comes from the Cognito access token API Gateway has already
  // verified, never from the body.
  const claims = event.requestContext.authorizer.jwt.claims;
  const userId = typeof claims.sub === 'string' ? claims.sub : '';
  const username = typeof claims.username === 'string' ? claims.username : '';
  if (!userId || !username) return { statusCode: 401, body: 'Missing identity' };

  const tier = readTier(event.body, event.isBase64Encoded);
  if (!tier) return { statusCode: 400, body: 'Body must be {"tier":"pro"|"team"}' };

  const email = await emailFor(username);
  if (!email) return { statusCode: 400, body: 'Account has no email address' };

  const stripe = await loadStripe();
  const priceId = PRICE_LOOKUP[tier];

  try {
    const customerId = await getOrCreateCustomer(stripe, userId, email);

    // The customer is stable per account, and Stripe will happily attach a second
    // subscription to the same customer. Nothing downstream detects that: the
    // webhook just rewrites the same DynamoDB item, so the user is billed twice
    // every month in silence. Send an existing subscriber to the billing portal
    // instead of into another checkout.
    //
    // The list filter takes one status at a time, so ask without it -- Stripe
    // then returns everything that has not been canceled -- and decide here.
    // A customer with more than 100 non-canceled subscriptions is already a
    // billing incident, and this guard is what stops that happening.
    const existing = await stripe.subscriptions.list({
      customer: customerId,
      limit: 100,
    });
    const live = existing.data.find((s) => isLiveSubscriptionStatus(s.status));
    if (live) {
      try {
        const portal = await stripe.billingPortal.sessions.create({
          customer: customerId,
          return_url: `https://${APP_DOMAIN}/`,
        });
        return {
          statusCode: 200,
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            url: portal.url,
            session_id: portal.id,
            already_subscribed: true,
            subscription_status: live.status,
          }),
        };
      } catch (err) {
        console.error(
          `Live subscription ${live.id} (${live.status}) exists but the billing portal is unavailable`,
          err,
        );
        return {
          statusCode: 409,
          body: `Already subscribed (status: ${live.status}). Manage or repair your plan from the Stripe billing portal.`,
        };
      }
    }

    const session = await stripe.checkout.sessions.create({
      mode: 'subscription',
      customer: customerId,
      line_items: [{ price: priceId, quantity: 1 }],
      client_reference_id: userId,
      subscription_data: {
        metadata: { userId, tier, project: 'jarvis' },
      },
      success_url: `https://${APP_DOMAIN}/payment/success?session_id={CHECKOUT_SESSION_ID}`,
      cancel_url: `https://${APP_DOMAIN}/payment/cancel`,
      automatic_tax: { enabled: false },
      allow_promotion_codes: true,
    }, {
      // A double click, a retry, or a second tab must reuse one session rather
      // than mint another. Stripe holds the key for 24h; a Checkout Session
      // expires in 24h too, so the window matches.
      idempotencyKey: `checkout:${userId}:${tier}`,
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
