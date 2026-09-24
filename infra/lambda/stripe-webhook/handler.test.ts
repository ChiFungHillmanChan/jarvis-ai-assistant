import test from 'node:test';
import assert from 'node:assert/strict';
import { createHarness } from '../../test-support/billing.cjs';

test('signature verification rejects a changed payload without changing billing state', async () => {
  const h=createHarness();const sub=h.seedSubscription();
  const event=h.webhookEvent('customer.subscription.updated',sub);
  event.body+=' ';
  assert.equal((await h.handler('stripe-webhook')(event)).statusCode,400);
  assert.equal(h.rows.get('user_fixture').tier,undefined);
});

test('an old active snapshot cannot restore a currently canceled subscription', async () => {
  const h=createHarness();const sub=h.seedSubscription('canceled');
  const event=h.webhookEvent('customer.subscription.updated',{...sub,status:'active'});
  const handler=h.handler('stripe-webhook');
  assert.equal((await handler(event)).statusCode,200);
  assert.equal((await handler(event)).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'free');
});

test('deletion of an old subscription keeps the currently active replacement', async () => {
  const h=createHarness();const old=h.seedSubscription('canceled','sub_old');
  h.subscriptions.set('sub_new',{...old,id:'sub_new',status:'active',created:2});
  h.invoices.get('in_fixture').subscription='sub_new';
  assert.equal((await h.handler('stripe-webhook')(h.webhookEvent('customer.subscription.deleted',old))).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'pro');
  assert.equal(h.rows.get('user_fixture').stripeSubscriptionId,'sub_new');
});

test('customer ownership mismatches are retriable and never grant a plan', async () => {
  const h=createHarness();const sub=h.seedSubscription();
  h.rows.set('user_fixture',{stripeCustomerId:'cus_other'});
  assert.equal((await h.handler('stripe-webhook')(h.webhookEvent('customer.subscription.updated',sub))).statusCode,500);
  assert.equal(h.rows.get('user_fixture').tier,undefined);
});

test('paid and failed invoices reconcile current state; failed writes are retried', async () => {
  const h=createHarness();const sub=h.seedSubscription();const handler=h.handler('stripe-webhook');
  h.failNextTierUpdate();
  const paid=h.webhookEvent('invoice.paid',h.invoices.get('in_fixture'));
  assert.equal((await handler(paid)).statusCode,500);
  assert.equal((await handler(paid)).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'pro');
  sub.status='unpaid';
  assert.equal((await handler(h.webhookEvent('invoice.payment_failed',h.invoices.get('in_fixture')))).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'free');
});

test('async checkout success reconciles the subscription and unpaid checkout does not grant it', async () => {
  const h=createHarness();const sub=h.seedSubscription();const handler=h.handler('stripe-webhook');
  h.invoices.get('in_fixture').paid=false;
  h.invoices.get('in_fixture').status='open';
  const session={id:'cs_async',mode:'subscription',client_reference_id:'user_fixture',customer:'cus_fixture',subscription:sub.id,payment_status:'unpaid',status:'complete'};
  h.sessions.set(session.id,session);
  assert.equal((await handler(h.webhookEvent('checkout.session.completed',session))).statusCode,200);
  assert.notEqual(h.rows.get('user_fixture').tier,'pro');
  session.payment_status='paid';h.invoices.get('in_fixture').paid=true;h.invoices.get('in_fixture').status='paid';
  assert.equal((await handler(h.webhookEvent('checkout.session.async_payment_succeeded',session))).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'pro');
});

test('unrelated hosted checkout without a client reference is acknowledged without writing user data', async () => {
  const h=createHarness();const sub=h.seedSubscription();sub.metadata={project:'another-product'};
  const session={id:'cs_other',mode:'subscription',customer:'cus_fixture',subscription:sub.id,client_reference_id:null};h.sessions.set(session.id,session);
  assert.equal((await h.handler('stripe-webhook')(h.webhookEvent('checkout.session.completed',session))).statusCode,200);
  assert.equal(h.calls.some(c=>c.kind==='UpdateCommand'),false);
});

test('current configured price determines upgrades and downgrades, regardless of the event snapshot', async () => {
  const h=createHarness({ENV:'prod',STRIPE_PRO_PRICE_ID:'price_pro_configured',STRIPE_TEAM_PRICE_ID:'price_team_configured'});
  const sub=h.seedSubscription('active','sub_fixture','price_pro_configured');
  const old={...sub,items:{data:[{price:{id:'price_team_configured'}}]}};
  const handler=h.handler('stripe-webhook');
  assert.equal((await handler(h.webhookEvent('customer.subscription.updated',old))).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'pro');
  sub.items.data[0].price.id='price_team_configured';
  assert.equal((await handler(h.webhookEvent('customer.subscription.updated',old))).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'team');
});

test('past_due keeps the existing grace policy, while async failure does not grant unpaid access', async () => {
  const h=createHarness();const sub=h.seedSubscription('past_due');const handler=h.handler('stripe-webhook');
  assert.equal((await handler(h.webhookEvent('invoice.payment_failed',h.invoices.get('in_fixture')))).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'pro');
  sub.status='active';h.invoices.get('in_fixture').paid=false;h.invoices.get('in_fixture').status='open';
  const session={id:'cs_failed',mode:'subscription',customer:'cus_fixture',client_reference_id:'user_fixture',subscription:sub.id};h.sessions.set(session.id,session);
  assert.equal((await handler(h.webhookEvent('checkout.session.async_payment_failed',session))).statusCode,200);
  assert.equal(h.rows.get('user_fixture').tier,'free');
});
