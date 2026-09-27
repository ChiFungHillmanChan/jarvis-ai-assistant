import test from 'node:test';
import assert from 'node:assert/strict';
import { paymentTemplate } from '../../test-support/payment-template.cjs';

function variables(template: any, functionName: string) {
  const fn = Object.values(template.Resources).find((r: any) => r.Type === 'AWS::Lambda::Function' && r.Properties.FunctionName === functionName) as any;
  assert.ok(fn, functionName);
  return fn.Properties.Environment.Variables;
}

test('the trial length reaches checkout and /me, defaulting to 28 days', () => {
  const template = paymentTemplate();
  assert.equal(variables(template, 'jarvis-sandbox-stripe-checkout').TRIAL_DAYS, '28');
  assert.equal(variables(template, 'jarvis-sandbox-me').TRIAL_DAYS, '28');
  assert.equal(variables(paymentTemplate({ trialDays: '61' }), 'jarvis-sandbox-stripe-checkout').TRIAL_DAYS, '61');
});

test('an invalid trial length stops synthesis before anything deploys', () => {
  for (const trialDays of ['0', '731', '2.5', 'four']) {
    assert.throws(() => paymentTemplate({ trialDays }), /trialDays/, trialDays);
  }
});
