import test from 'node:test';
import assert from 'node:assert/strict';
import { createHarness } from '../../test-support/billing.cjs';

test('configured prices are used and production refuses missing price configuration', async () => {
  const configured = createHarness({ENV:'prod',STRIPE_PRO_PRICE_ID:'price_real_pro',STRIPE_TEAM_PRICE_ID:'price_real_team'});
  assert.equal((await configured.handler('stripe-checkout')(configured.request())).statusCode,200);
  assert.equal(configured.calls.find(c=>c.kind==='checkout.create').input.line_items[0].price,'price_real_pro');
  const missing=createHarness({ENV:'prod'});
  assert.equal((await missing.handler('stripe-checkout')(missing.request())).statusCode,503);
  assert.equal(missing.calls.some(c=>c.kind==='checkout.create'),false);
});

test('concurrent first requests create one customer and one open checkout', async () => {
  const h=createHarness();const handler=h.handler('stripe-checkout');
  const responses=await Promise.all([handler(h.request('pro')),handler(h.request('team'))]);
  assert.equal(responses.filter(r=>r.statusCode===200).length,1);
  assert.equal(responses.filter(r=>r.statusCode===409).length,1);
  assert.equal(h.customers.size,1);
  assert.equal([...h.sessions.values()].filter(s=>s.status==='open').length,1);
  assert.ok(h.calls.find(c=>c.kind==='customer.create').options.idempotencyKey);
});

test('same-tier retry reuses a session; changing tier expires the old session first', async () => {
  const h=createHarness();const handler=h.handler('stripe-checkout');
  const first=JSON.parse((await handler(h.request('pro'))).body);
  const retry=JSON.parse((await handler(h.request('pro'))).body);
  assert.equal(first.session_id,retry.session_id);
  const changed=JSON.parse((await handler(h.request('team'))).body);
  assert.notEqual(changed.session_id,first.session_id);
  assert.equal(h.sessions.get(first.session_id).status,'expired');
  assert.equal([...h.sessions.values()].filter(s=>s.status==='open').length,1);
});

test('a live or past-due subscription goes to the portal without another checkout', async () => {
  for(const status of ['active','past_due','unpaid','paused','incomplete','trialing']) {
    const h=createHarness();h.seedSubscription(status);
    const response=await h.handler('stripe-checkout')(h.request());
    assert.equal(response.statusCode,200);
    assert.equal(JSON.parse(response.body).already_subscribed,true);
    assert.equal(h.sessions.size,0);
  }
});

test('retries recover an ambiguous customer and checkout before changing tier', async () => {
  const h=createHarness();const handler=h.handler('stripe-checkout');
  h.failNextUpdateContaining('SET stripeCustomerId');
  assert.equal((await handler(h.request())).statusCode,500);
  assert.equal(h.customers.size,1);
  h.failNextUpdateContaining('SET checkoutSessionId');
  assert.equal((await handler(h.request())).statusCode,500);
  assert.equal(h.customers.size,1);
  const first=[...h.sessions.values()][0];
  assert.equal((await handler(h.request('team'))).statusCode,200);
  assert.equal(h.customers.size,1);
  assert.equal(h.sessions.get(first.id).status,'expired');
  assert.equal([...h.sessions.values()].filter(s=>s.status==='open').length,1);
});

test('a legacy open session is expired before a new checkout is created', async () => {
  const h=createHarness();h.seedSubscription('canceled');
  h.sessions.set('cs_legacy',{id:'cs_legacy',mode:'subscription',customer:'cus_fixture',client_reference_id:'user_fixture',status:'open'});
  assert.equal((await h.handler('stripe-checkout')(h.request('team'))).statusCode,200);
  assert.equal(h.sessions.get('cs_legacy').status,'expired');
  assert.equal([...h.sessions.values()].filter(s=>s.status==='open').length,1);
});

test('browser completion during expiry blocks the replacement checkout', async () => {
  const h=createHarness();const handler=h.handler('stripe-checkout');
  await handler(h.request());
  h.stripe.checkout.sessions.expire=async id=>{h.sessions.get(id).status='complete';throw new Error('Completed in browser');};
  assert.equal((await handler(h.request('team'))).statusCode,500);
  assert.equal(h.sessions.size,1);
  assert.equal((await handler(h.request('team'))).statusCode,409);
  assert.equal(h.sessions.size,1);
});

test('checkout returns to the selected allowlisted locale and preserves retry parameters', async () => {
  const h=createHarness();const handler=h.handler('stripe-checkout');
  const request=h.request();request.body=JSON.stringify({tier:'pro',locale:'zh-HK'});
  const first=await handler(request);
  assert.equal(first.statusCode,200);
  const session=h.calls.find(c=>c.kind==='checkout.create').input;
  assert.equal(session.locale,'zh-HK');
  assert.equal(session.success_url,'https://example.test/zh-HK/payment/success?session_id={CHECKOUT_SESSION_ID}');
  assert.equal(session.cancel_url,'https://example.test/zh-HK/payment/cancel');
  assert.equal(JSON.parse((await handler(h.request())).body).session_id,JSON.parse(first.body).session_id);
});

test('new checkout page language defaults to English for missing or unsupported locales', async () => {
  for (const locale of [undefined,'en','//evil.test']) {
    const h=createHarness();const request=h.request();request.body=JSON.stringify({tier:'pro',locale});
    assert.equal((await h.handler('stripe-checkout')(request)).statusCode,200);
    assert.equal(h.calls.find(c=>c.kind==='checkout.create').input.locale,'en');
  }
});

test('an ambiguous new checkout keeps its original Stripe locale when the app language changes', async () => {
  const h=createHarness();const handler=h.handler('stripe-checkout');
  const request=h.request();request.body=JSON.stringify({tier:'pro',locale:'zh-HK'});
  h.failNextUpdateContaining('SET checkoutSessionId');
  assert.equal((await handler(request)).statusCode,500);
  assert.equal((await handler(h.request())).statusCode,200);
  const calls=h.calls.filter(c=>c.kind==='checkout.create');
  assert.equal(calls.length,2);
  assert.equal(calls[0].input.locale,'zh-HK');
  assert.deepEqual(calls[1].input,calls[0].input);
  assert.equal(calls[1].options.idempotencyKey,calls[0].options.idempotencyKey);
  assert.equal(h.sessions.size,1);
});

test('a legacy ambiguous checkout recovers without adding a locale to its existing idempotency key', async () => {
  const h=createHarness();h.seedSubscription('canceled');
  const input={
    mode:'subscription',customer:'cus_fixture',
    line_items:[{price:'price_1TUsAuJCG4qg3rNcZVpOopcD',quantity:1}],
    client_reference_id:'user_fixture',
    subscription_data:{metadata:{userId:'user_fixture',tier:'pro',project:'jarvis'}},
    success_url:'https://example.test/zh-HK/payment/success?session_id={CHECKOUT_SESSION_ID}',
    cancel_url:'https://example.test/zh-HK/payment/cancel',
    automatic_tax:{enabled:false},allow_promotion_codes:true,
  };
  const original=await h.stripe.checkout.sessions.create(input,{idempotencyKey:'checkout:sandbox:user_fixture:legacy-attempt'});
  h.rows.set('user_fixture',{
    stripeCustomerId:'cus_fixture',checkoutAttemptId:'legacy-attempt',checkoutAttemptAt:Date.now(),
    checkoutTier:'pro',checkoutPriceId:input.line_items[0].price,checkoutLocale:'zh-HK',
  });
  const response=await h.handler('stripe-checkout')(h.request());
  assert.equal(response.statusCode,200);
  assert.equal(JSON.parse(response.body).session_id,original.id);
  const retry=h.calls.filter(c=>c.kind==='checkout.create').at(-1);
  assert.deepEqual(retry.input,input);
  assert.equal(Object.hasOwn(retry.input,'locale'),false);
  assert.equal(h.sessions.size,1);
});

test('a canceled completed subscription can be purchased again', async () => {
  const h=createHarness();const handler=h.handler('stripe-checkout');
  const first=JSON.parse((await handler(h.request())).body);
  const session=h.sessions.get(first.session_id);
  session.status='complete';session.subscription='sub_canceled';
  h.subscriptions.set('sub_canceled',{id:'sub_canceled',customer:session.customer,status:'canceled',metadata:{userId:'user_fixture',project:'jarvis'}});
  const next=await handler(h.request());
  assert.equal(next.statusCode,200);
  assert.notEqual(JSON.parse(next.body).session_id,first.session_id);
});

test('a subscription completed between the initial list and session check prevents a new checkout', async () => {
  const h=createHarness();h.seedSubscription('canceled');
  const original=h.stripe.checkout.sessions.list;
  h.stripe.checkout.sessions.list=async input=>{h.subscriptions.get('sub_fixture').status='active';return original(input);};
  const response=await h.handler('stripe-checkout')(h.request());
  assert.equal(response.statusCode,409);
  assert.equal(h.sessions.size,0);
});

test('a first checkout offers the configured trial and still collects a card', async () => {
  for (const [configured, expected] of [[undefined, 28], ['61', 61]] as const) {
    const h=createHarness(configured ? {TRIAL_DAYS:configured} : {});
    assert.equal((await h.handler('stripe-checkout')(h.request())).statusCode,200);
    const input=h.calls.find(c=>c.kind==='checkout.create').input;
    assert.equal(input.subscription_data.trial_period_days,expected);
    assert.equal(Object.hasOwn(input,'payment_method_collection'),false,'Stripe default: always collect a card');
    assert.equal(input.allow_promotion_codes,true);
  }
});

test('an account that has subscribed before gets no second trial', async () => {
  const h=createHarness();h.seedSubscription('canceled');
  assert.equal((await h.handler('stripe-checkout')(h.request())).statusCode,200);
  assert.equal(Object.hasOwn(h.calls.find(c=>c.kind==='checkout.create').input.subscription_data,'trial_period_days'),false);
});

test('a subscription that never started does not use up the trial', async () => {
  const h=createHarness();h.seedSubscription('incomplete_expired');
  assert.equal((await h.handler('stripe-checkout')(h.request())).statusCode,200);
  assert.equal(h.calls.find(c=>c.kind==='checkout.create').input.subscription_data.trial_period_days,28);
});

test('a misconfigured trial length refuses checkout before touching Stripe', async () => {
  const h=createHarness({TRIAL_DAYS:'0'});
  const response=await h.handler('stripe-checkout')(h.request());
  assert.equal(response.statusCode,503);
  assert.equal(JSON.parse(response.body).error,'billing_trial_not_configured');
  assert.equal(h.customers.size,0);
});
