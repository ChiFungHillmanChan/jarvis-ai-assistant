import test from 'node:test';
import assert from 'node:assert/strict';
import { createHarness } from '../../test-support/billing.cjs';

test('portal uses only the verified account customer, ignoring supplied identity', async () => {
  const h=createHarness();h.seedSubscription();
  const event=h.request();event.body=JSON.stringify({userId:'another',customer:'cus_other'});
  const response=await h.handler('stripe-portal')(event);
  assert.equal(response.statusCode,200);
  assert.equal(h.calls.find(c=>c.kind==='portal.create').input.customer,'cus_fixture');
  assert.equal(JSON.parse(response.body).url,'https://billing.stripe.com/p/session/fixture');
});

test('portal rejects missing identity and customers without a billing record', async () => {
  const h=createHarness();const handler=h.handler('stripe-portal');
  const event=h.request();event.requestContext.authorizer.jwt.claims={};
  assert.equal((await handler(event)).statusCode,401);
  assert.equal((await handler(h.request())).statusCode,404);
  assert.equal(h.calls.some(c=>c.kind==='portal.create'),false);
});

test('portal return locale is allowlisted and defaults to English', async () => {
  for (const locale of ['zh-HK','en','//evil.test']) {
    const h=createHarness();h.seedSubscription();const event=h.request();event.body=JSON.stringify({locale});
    assert.equal((await h.handler('stripe-portal')(event)).statusCode,200);
    assert.equal(h.calls.find(c=>c.kind==='portal.create').input.return_url,`https://example.test/${locale==='zh-HK'?'zh-HK':'en'}/`);
    assert.equal(h.calls.find(c=>c.kind==='portal.create').input.locale,locale==='zh-HK'?'zh-HK':'en');
  }
});

test('portal uses the JARVIS configuration and production refuses an unspecified portal', async () => {
  const configured=createHarness({ENV:'prod',STRIPE_PORTAL_CONFIGURATION_ID:'bpc_jarvis'});configured.seedSubscription();
  assert.equal((await configured.handler('stripe-portal')(configured.request())).statusCode,200);
  assert.equal(configured.calls.find(c=>c.kind==='portal.create').input.configuration,'bpc_jarvis');
  const missing=createHarness({ENV:'prod'});missing.seedSubscription();
  assert.equal((await missing.handler('stripe-portal')(missing.request())).statusCode,503);
  assert.equal(missing.calls.some(c=>c.kind==='portal.create'),false);
});
