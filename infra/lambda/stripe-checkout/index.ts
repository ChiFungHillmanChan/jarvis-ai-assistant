import type { APIGatewayProxyEventV2WithJWTAuthorizer, APIGatewayProxyResultV2 } from 'aws-lambda';
import { randomUUID } from 'node:crypto';
import { AdminGetUserCommand, CognitoIdentityProviderClient } from '@aws-sdk/client-cognito-identity-provider';
import type Stripe from 'stripe';
import { readCode, readLocale, readTier } from './body';
import { appDomain, BillingError, failure, isLiveSubscriptionStatus, json, loadBilling, objectId, portalSession, prices, subscriptionsFor, updateUser, userRow, verifyCustomer, withBillingLock } from './billing';
import { readTrialDays } from './trial';
export { isLiveSubscriptionStatus } from './billing';

const cognito = new CognitoIdentityProviderClient({});
const RECOVERY_WINDOW_MS = 23 * 60 * 60 * 1000;

async function customerFor(stripe: Stripe, userId: string, username: string, owner: string) {
  let row = await userRow(userId);
  if (row.stripeCustomerId) {
    await verifyCustomer(stripe, row.stripeCustomerId, userId);
    return row.stripeCustomerId as string;
  }
  if (!row.customerAttemptAt) {
    const account = await cognito.send(new AdminGetUserCommand({ UserPoolId: process.env.USER_POOL_ID!, Username: username }));
    const email = account.UserAttributes?.find(a => a.Name === 'email')?.Value;
    if (!email) throw new BillingError('account_email_missing', 400);
    await updateUser(userId, 'SET customerAttemptAt = :at, customerAttemptEmail = :email', { ':at': Date.now(), ':email': email }, owner);
    row = await userRow(userId);
  }
  // After Stripe's idempotency window an ambiguous create cannot safely be
  // repeated. Keep the record for reconciliation instead of risking billing
  // the same account under another customer.
  if (Date.now() - row.customerAttemptAt > RECOVERY_WINDOW_MS) throw new BillingError('billing_customer_reconciliation_required', 503);
  const customer = await stripe.customers.create({ email: row.customerAttemptEmail, metadata: { userId, project: 'jarvis' } }, {
    idempotencyKey: `customer:${process.env.ENV}:${userId}`,
  });
  await updateUser(userId, 'SET stripeCustomerId = :customer, email = :email REMOVE customerAttemptAt, customerAttemptEmail', { ':customer': customer.id, ':email': row.customerAttemptEmail }, owner);
  return customer.id;
}

async function createAttempt(stripe: Stripe, userId: string, customer: string, row: Record<string, any>) {
  if (Date.now() - row.checkoutAttemptAt > RECOVERY_WINDOW_MS) throw new BillingError('billing_checkout_reconciliation_required', 503);
  // Older durable attempts omitted Stripe's locale parameter. Preserve that
  // exact request on recovery; adding it would change an existing idempotency key.
  const stripeLocale = row.checkoutStripeLocale === 'zh-HK' ? 'zh-HK' : row.checkoutStripeLocale === 'en' ? 'en' : undefined;
  // Attempts written before trials existed carry no trial length and replay without one.
  const trialDays = attemptTrialDays(row);
  const promotionCode = typeof row.checkoutPromotionCodeId === 'string' ? row.checkoutPromotionCodeId : undefined;
  return stripe.checkout.sessions.create({
    mode: 'subscription', customer,
    line_items: [{ price: row.checkoutPriceId, quantity: 1 }],
    client_reference_id: userId,
    subscription_data: { metadata: { userId, tier: row.checkoutTier, project: 'jarvis' }, ...(trialDays ? { trial_period_days: trialDays } : {}) },
    success_url: `https://${appDomain}/${row.checkoutLocale || 'en'}/payment/success?session_id={CHECKOUT_SESSION_ID}`,
    cancel_url: `https://${appDomain}/${row.checkoutLocale || 'en'}/payment/cancel`,
    automatic_tax: { enabled: false },
    // An account can default Checkout to Managed Payments (Stripe as the
    // seller), which rejects automatic_tax: false. JARVIS sells directly.
    // Attempts written before this was sent replay without it. The installed
    // SDK's types predate the parameter.
    ...(row.checkoutSelfBilled ? ({ managed_payments: { enabled: false } } as object) : {}),
    // A code checkout applies the discount itself and asks for a card only if
    // something is due; Stripe rejects allow_promotion_codes alongside discounts.
    ...(promotionCode
      ? { discounts: [{ promotion_code: promotionCode }], payment_method_collection: 'if_required' as const }
      : { allow_promotion_codes: true }),
    ...(stripeLocale ? { locale: stripeLocale } : {}),
  }, { idempotencyKey: `checkout:${process.env.ENV}:${userId}:${row.checkoutAttemptId}` });
}

function attemptTrialDays(row: Record<string, any>): number {
  return typeof row.checkoutTrialDays === 'number' && row.checkoutTrialDays > 0 ? row.checkoutTrialDays : 0;
}

const ATTEMPT_FIELDS = 'checkoutAttemptId, checkoutAttemptAt, checkoutTier, checkoutPriceId, checkoutLocale, checkoutStripeLocale, checkoutTrialDays, checkoutPromotionCodeId, checkoutSelfBilled, checkoutSessionId';

async function activePromotionCode(stripe: Stripe, code: string) {
  const page = await stripe.promotionCodes.list({ code, active: true, limit: 1 });
  return page.data[0] ?? null;
}

// Stripe refusing a discount (expired, redeemed out, restricted) is a definite
// answer: no session exists under this attempt, so it is cleared and the app
// shows "code not recognised". Any other failure keeps the attempt for recovery.
async function createOrRejectCode(stripe: Stripe, userId: string, customer: string, row: Record<string, any>, owner: string) {
  try {
    return await createAttempt(stripe, userId, customer, row);
  } catch (error) {
    if (typeof row.checkoutPromotionCodeId === 'string' && (error as { type?: string })?.type === 'StripeInvalidRequestError') {
      const stripeError = error as { code?: string; param?: string };
      console.warn('Promotion code refused by Stripe', { code: stripeError.code, param: stripeError.param });
      await updateUser(userId, `REMOVE ${ATTEMPT_FIELDS}`, {}, owner);
      throw new BillingError('invalid_code', 400);
    }
    throw error;
  }
}

// Include sessions created before durable attempt tracking was introduced. Do
// not leave a legacy browser tab able to purchase another subscription.
async function expireOtherSessions(stripe: Stripe, customer: string, userId: string, keep?: string) {
  const pending: Stripe.Checkout.Session[] = [];
  let after: string | undefined;
  do {
    const page = await stripe.checkout.sessions.list({ customer, status: 'open', limit: 100, ...(after ? { starting_after: after } : {}) });
    pending.push(...page.data);
    after = page.has_more ? page.data.at(-1)?.id : undefined;
    if (page.has_more && !after) throw new BillingError('billing_checkout_page_invalid');
  } while (after);
  for (const session of pending) {
    if (session.mode !== 'subscription' || session.id === keep) continue;
    if (session.client_reference_id !== userId) throw new BillingError('billing_checkout_mismatch');
    await stripe.checkout.sessions.expire(session.id);
  }
}

export async function handler(event: APIGatewayProxyEventV2WithJWTAuthorizer): Promise<APIGatewayProxyResultV2> {
  const claims = event.requestContext.authorizer?.jwt?.claims || {};
  const userId = typeof claims.sub === 'string' ? claims.sub : '';
  const username = typeof claims.username === 'string' ? claims.username : '';
  if (!userId || !username) return json(401, { error: 'missing_identity' });
  const tier = readTier(event.body, event.isBase64Encoded);
  if (!tier) return json(400, { error: 'invalid_tier' });
  const code = readCode(event.body, event.isBase64Encoded);
  if (code === false) return json(400, { error: 'invalid_code' });
  try {
    const configuredPrices = prices();
    const configuredTrialDays = readTrialDays(process.env.TRIAL_DAYS);
    if (configuredTrialDays === null) throw new BillingError('billing_trial_not_configured', 503);
    const { stripe } = await loadBilling();
    return await withBillingLock(userId, async owner => {
      const customer = await customerFor(stripe, userId, username, owner);
      const history = await subscriptionsFor(stripe, customer);
      const live = history.find(s => isLiveSubscriptionStatus(s.status));
      if (live) {
        await expireOtherSessions(stripe, customer, userId);
        const portal = await portalSession(stripe, customer, readLocale(event.body, event.isBase64Encoded));
        return json(200, { url: portal.url, session_id: portal.id, already_subscribed: true, subscription_status: live.status });
      }
      // The trial is for first-time subscribers. A subscription that never
      // started (incomplete_expired) does not use it up.
      // Resolve the code before recovering a pending attempt, so an open
      // session is reused only for exactly the same terms.
      const promotionCode = code ? await activePromotionCode(stripe, code) : null;
      if (code && !promotionCode) throw new BillingError('invalid_code', 400);
      const trialDays = promotionCode || history.some(s => s.status !== 'incomplete_expired') ? 0 : configuredTrialDays;
      let row = await userRow(userId);
      if (row.checkoutAttemptId) {
        // Recover an ambiguous create before permitting a different tier. The
        // durable attempt keeps exactly the same Stripe parameters on retries.
        const sessionId = row.checkoutSessionId || (await createOrRejectCode(stripe, userId, customer, row, owner)).id;
        const pending = await stripe.checkout.sessions.retrieve(sessionId);
        if (pending.customer !== customer || pending.client_reference_id !== userId) throw new BillingError('billing_checkout_mismatch');
        if (!row.checkoutSessionId) await updateUser(userId, 'SET checkoutSessionId = :session', { ':session': pending.id }, owner);
        await expireOtherSessions(stripe, customer, userId, pending.id);
        if (pending.status === 'complete') {
          const subscriptionId = objectId(pending.subscription);
          if (!subscriptionId) throw new BillingError('billing_checkout_processing', 409);
          const completed = await stripe.subscriptions.retrieve(subscriptionId);
          if (objectId(completed.customer) !== customer || completed.metadata.userId !== userId) throw new BillingError('billing_subscription_mismatch');
          if (isLiveSubscriptionStatus(completed.status)) throw new BillingError('billing_checkout_processing', 409);
        }
        if (pending.status === 'open') {
          if (row.checkoutTier === tier && row.checkoutPriceId === configuredPrices[tier] && attemptTrialDays(row) === trialDays
            && (row.checkoutPromotionCodeId ?? null) === (promotionCode?.id ?? null)) return json(200, { url: pending.url, session_id: pending.id });
          // A concurrent browser completion makes expire fail, so no replacement
          // checkout is created until a subsequent request reconciles it.
          await stripe.checkout.sessions.expire(pending.id);
        } else if (pending.status !== 'expired' && pending.status !== 'complete') throw new BillingError('billing_checkout_processing', 409);
      }
      await expireOtherSessions(stripe, customer, userId);
      // A legacy browser session may complete after the first subscription list
      // but before it appears in the open-session list. Recheck after expiring
      // every remaining open session, when no pending tab can complete later.
      if ((await subscriptionsFor(stripe, customer)).some(s => isLiveSubscriptionStatus(s.status))) throw new BillingError('billing_checkout_processing', 409);
      // DynamoDB rejects unused expression values, so the code is set or removed explicitly.
      const codeSet = promotionCode ? ', checkoutPromotionCodeId = :promotionCode' : '';
      const codeRemove = promotionCode ? '' : ', checkoutPromotionCodeId';
      await updateUser(userId,
        `SET checkoutAttemptId = :attempt, checkoutAttemptAt = :at, checkoutTier = :tier, checkoutPriceId = :price, checkoutLocale = :locale, checkoutStripeLocale = :locale, checkoutTrialDays = :trialDays, checkoutSelfBilled = :selfBilled${codeSet} REMOVE checkoutSessionId${codeRemove}`,
        { ':attempt': randomUUID(), ':at': Date.now(), ':tier': tier, ':price': configuredPrices[tier], ':locale': readLocale(event.body, event.isBase64Encoded), ':trialDays': trialDays, ':selfBilled': true, ...(promotionCode ? { ':promotionCode': promotionCode.id } : {}) }, owner);
      row = await userRow(userId);
      const session = await createOrRejectCode(stripe, userId, customer, row, owner);
      await updateUser(userId, 'SET checkoutSessionId = :session', { ':session': session.id }, owner);
      return json(200, { url: session.url, session_id: session.id });
    });
  } catch (error) { return failure(error); }
}
