import type { APIGatewayProxyEventV2, APIGatewayProxyResultV2 } from 'aws-lambda';
import type Stripe from 'stripe';
import { BillingError, failure, json, loadBilling, objectId, prices, subscriptionsFor, updateUser, userRow, verifyCustomer, withBillingLock } from '../stripe-checkout/billing';
import type { Tier } from '../stripe-checkout/billing';

export function entitledTier(priceId: string | null | undefined, status: Stripe.Subscription.Status): Tier {
  if (!['active', 'trialing', 'past_due'].includes(status)) return 'free';
  const configured = prices();
  if (priceId === configured.pro) return 'pro';
  if (priceId === configured.team) return 'team';
  // A new/misconfigured price must be reconciled, not silently turn a paid
  // subscriber into Free or acknowledge an event that cannot be applied.
  throw new BillingError('billing_unknown_price');
}

async function paidTier(stripe: Stripe, subscription: Stripe.Subscription): Promise<Tier> {
  const tier = entitledTier(subscription.items.data[0]?.price.id, subscription.status);
  if (tier === 'free' || subscription.status !== 'active') return tier;
  const invoiceId = objectId(subscription.latest_invoice);
  if (!invoiceId) return 'free';
  const invoice = await stripe.invoices.retrieve(invoiceId);
  if (objectId(invoice.customer) !== objectId(subscription.customer) || objectId(invoice.subscription) !== subscription.id) {
    throw new BillingError('billing_invoice_mismatch');
  }
  // Asynchronous Checkout methods can report an active subscription before the
  // first invoice is paid. Never provision from the Checkout snapshot alone.
  return invoice.paid || invoice.status === 'paid' ? tier : 'free';
}

async function eventSubscription(stripe: Stripe, event: Stripe.Event): Promise<Stripe.Subscription | null> {
  let subscriptionId: string | null = null;
  let expectedCustomer: string | null = null;
  let expectedUser: string | null = null;
  let checkoutIdentityRequired = false;
  switch (event.type) {
    case 'customer.subscription.created':
    case 'customer.subscription.updated':
    case 'customer.subscription.deleted':
    case 'customer.subscription.paused':
    case 'customer.subscription.resumed':
      subscriptionId = event.data.object.id;
      break;
    case 'checkout.session.completed':
    case 'checkout.session.async_payment_succeeded':
    case 'checkout.session.async_payment_failed': {
      const session = await stripe.checkout.sessions.retrieve(event.data.object.id);
      if (session.mode !== 'subscription') return null;
      subscriptionId = objectId(session.subscription);
      expectedCustomer = objectId(session.customer);
      expectedUser = session.client_reference_id;
      if (!subscriptionId) return null;
      checkoutIdentityRequired = true;
      break;
    }
    case 'invoice.paid':
    case 'invoice.payment_failed': {
      const invoice = await stripe.invoices.retrieve(event.data.object.id);
      subscriptionId = objectId(invoice.subscription);
      if (!subscriptionId) return null;
      expectedCustomer = objectId(invoice.customer);
      break;
    }
    default: return null;
  }
  const current = await stripe.subscriptions.retrieve(subscriptionId!);
  // A shared Stripe account can send events for other products, whose
  // Checkouts do not carry JARVIS identity fields. Inspect the current project
  // before enforcing our identity contract.
  if (current.metadata.project !== 'jarvis') return null;
  if (checkoutIdentityRequired && (!expectedCustomer || !expectedUser)) throw new BillingError('billing_checkout_incomplete');
  if ((expectedCustomer && expectedCustomer !== objectId(current.customer)) || (expectedUser && expectedUser !== current.metadata.userId)) {
    throw new BillingError('billing_event_identity_mismatch');
  }
  return current;
}

async function reconcile(stripe: Stripe, event: Stripe.Event) {
  const initial = await eventSubscription(stripe, event);
  if (!initial || initial.metadata.project !== 'jarvis') return;
  const userId = initial.metadata.userId;
  if (!userId) throw new BillingError('billing_subscription_identity_missing');
  await withBillingLock(userId, async owner => {
    // Stripe explicitly does not guarantee event ordering. Refetch *inside*
    // the account lock; event.created (second precision) is not an ordering key.
    const current = await stripe.subscriptions.retrieve(initial.id);
    const customer = objectId(current.customer);
    if (!customer || current.metadata.userId !== userId || current.metadata.project !== 'jarvis') throw new BillingError('billing_subscription_mismatch');
    const row = await userRow(userId);
    if (row.stripeCustomerId !== customer) throw new BillingError('billing_customer_mismatch');
    await verifyCustomer(stripe, customer, userId);
    const subscriptions = await subscriptionsFor(stripe, customer);
    // Include a terminal subscription even if a concurrently updated list omits
    // it; an old deletion must not revoke a newer replacement subscription.
    const candidates = subscriptions.filter(s => s.id !== current.id);
    candidates.push(current);
    let selected: Stripe.Subscription = current;
    let selectedTier: Tier = 'free';
    const rank = { free: 0, pro: 1, team: 2 };
    for (const subscription of candidates) {
      if (subscription.metadata.project !== 'jarvis') continue;
      if (subscription.metadata.userId !== userId || objectId(subscription.customer) !== customer) throw new BillingError('billing_subscription_mismatch');
      const tier = await paidTier(stripe, subscription);
      if (rank[tier] > rank[selectedTier] || (tier === selectedTier && subscription.created > selected.created)) {
        selected = subscription;
        selectedTier = tier;
      }
    }
    // Replays perform this same current-state reconciliation. No event snapshot
    // can restore a canceled plan, and no outbound payment occurs in a webhook.
    await updateUser(userId,
      'SET tier = :tier, stripeSubscriptionId = :subscription, subscriptionStatus = :status, updatedAt = :updated, lastStripeEventId = :event',
      { ':tier': selectedTier, ':subscription': selected.id, ':status': selected.status, ':updated': new Date().toISOString(), ':event': event.id }, owner);
  });
}

export async function handler(event: APIGatewayProxyEventV2): Promise<APIGatewayProxyResultV2> {
  const signature = event.headers['stripe-signature'] || event.headers['Stripe-Signature'];
  if (!signature || !event.body) return json(400, { error: 'missing_signature_or_body' });
  let billing;
  try { billing = await loadBilling(); }
  catch (error) { return failure(error); }
  const rawBody = event.isBase64Encoded ? Buffer.from(event.body, 'base64').toString('utf8') : event.body;
  let stripeEvent: Stripe.Event;
  try { stripeEvent = billing.stripe.webhooks.constructEvent(rawBody, signature, billing.webhookSecret); }
  catch { return json(400, { error: 'invalid_signature' }); }
  try {
    await reconcile(billing.stripe, stripeEvent);
    return json(200, { received: true });
  } catch (error) {
    // Any fetch, ownership, configuration, lease or write failure stays non-2xx
    // so Stripe retries instead of acknowledging an unapplied entitlement.
    return failure(error);
  }
}
