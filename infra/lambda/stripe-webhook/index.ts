import type { APIGatewayProxyEventV2, APIGatewayProxyResultV2 } from 'aws-lambda';
import { SecretsManagerClient, GetSecretValueCommand } from '@aws-sdk/client-secrets-manager';
import { DynamoDBClient } from '@aws-sdk/client-dynamodb';
import { DynamoDBDocumentClient, UpdateCommand } from '@aws-sdk/lib-dynamodb';
import Stripe from 'stripe';

const sm = new SecretsManagerClient({});
const ddb = DynamoDBDocumentClient.from(new DynamoDBClient({}));

let cachedSecrets: { webhookSecret: string; apiKey: string } | null = null;
let cachedStripe: Stripe | null = null;

async function loadSecrets() {
  if (cachedSecrets) return cachedSecrets;
  const out = await sm.send(new GetSecretValueCommand({ SecretId: process.env.STRIPE_SECRET_NAME! }));
  cachedSecrets = JSON.parse(out.SecretString!);
  cachedStripe = new Stripe(cachedSecrets!.apiKey, { apiVersion: '2024-12-18.acacia' });
  return cachedSecrets!;
}

// Price IDs created in JARVIS test mode (HKD).
// LIVE mode prices will need a separate mapping via env var.
const PRICE_TIER_MAP: Record<string, 'pro' | 'team'> = {
  // sandbox / test mode
  'price_1TUsAuJCG4qg3rNcZVpOopcD': 'pro',
  'price_1TUsAwJCG4qg3rNcPY3YJiOB': 'team',
};

function tierForPriceId(priceId: string | null | undefined): 'pro' | 'team' | 'free' {
  if (!priceId) return 'free';
  return PRICE_TIER_MAP[priceId] ?? 'free';
}

async function setUserTier(userId: string, tier: 'free' | 'pro' | 'team', stripeCustomerId: string | null, status: string) {
  await ddb.send(new UpdateCommand({
    TableName: process.env.USER_META_TABLE!,
    Key: { userId },
    UpdateExpression: 'SET tier = :t, stripeCustomerId = :c, subscriptionStatus = :s, updatedAt = :u',
    ExpressionAttributeValues: {
      ':t': tier,
      ':c': stripeCustomerId,
      ':s': status,
      ':u': new Date().toISOString(),
    },
  }));
}

export async function handler(event: APIGatewayProxyEventV2): Promise<APIGatewayProxyResultV2> {
  const sig = event.headers['stripe-signature'];
  if (!sig) {
    return { statusCode: 400, body: 'Missing stripe-signature' };
  }

  let secrets;
  try {
    secrets = await loadSecrets();
  } catch (err) {
    console.error('Failed to load Stripe secrets', err);
    return { statusCode: 500, body: 'Configuration error' };
  }

  const rawBody = event.isBase64Encoded ? Buffer.from(event.body!, 'base64').toString('utf8') : event.body!;

  let stripeEvent: Stripe.Event;
  try {
    stripeEvent = cachedStripe!.webhooks.constructEvent(rawBody, sig, secrets.webhookSecret);
  } catch (err) {
    console.error('Webhook signature verification failed', err);
    return { statusCode: 400, body: `Webhook Error: ${err instanceof Error ? err.message : 'unknown'}` };
  }

  console.log(`Processing Stripe event: ${stripeEvent.type} ${stripeEvent.id}`);

  try {
    switch (stripeEvent.type) {
      case 'checkout.session.completed': {
        const session = stripeEvent.data.object as Stripe.Checkout.Session;
        const userId = session.client_reference_id;
        if (!userId) {
          console.warn('checkout.session.completed without client_reference_id; skipping');
          break;
        }
        if (!session.subscription) {
          console.warn(`checkout.session.completed without subscription (mode=${session.mode}); skipping`);
          break;
        }
        const subscription = await cachedStripe!.subscriptions.retrieve(session.subscription as string);
        const priceId = subscription.items.data[0]?.price.id;
        await setUserTier(userId, tierForPriceId(priceId), session.customer as string, subscription.status);
        break;
      }
      case 'customer.subscription.updated':
      case 'customer.subscription.created': {
        const sub = stripeEvent.data.object as Stripe.Subscription;
        const userId = (sub.metadata && sub.metadata.userId) || null;
        if (!userId) break;
        const priceId = sub.items.data[0]?.price.id;
        await setUserTier(userId, tierForPriceId(priceId), sub.customer as string, sub.status);
        break;
      }
      case 'customer.subscription.deleted': {
        const sub = stripeEvent.data.object as Stripe.Subscription;
        const userId = (sub.metadata && sub.metadata.userId) || null;
        if (!userId) break;
        await setUserTier(userId, 'free', sub.customer as string, 'canceled');
        break;
      }
      case 'invoice.payment_failed': {
        const invoice = stripeEvent.data.object as Stripe.Invoice;
        console.warn(`Payment failed for customer ${invoice.customer}, invoice ${invoice.id}`);
        break;
      }
      default:
        console.log(`Unhandled event type ${stripeEvent.type}`);
    }
  } catch (err) {
    console.error('Error processing Stripe event', err);
    return { statusCode: 500, body: 'Internal error' };
  }

  return { statusCode: 200, body: JSON.stringify({ received: true }) };
}
