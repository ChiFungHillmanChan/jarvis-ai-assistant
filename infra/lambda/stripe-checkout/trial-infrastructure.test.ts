import test from 'node:test';
import assert from 'node:assert/strict';
import { paymentTemplate } from '../../test-support/payment-template.cjs';
import { DEFAULT_TRIAL_DAYS, MAX_TRIAL_DAYS } from './trial.ts';

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

test('CDK validation stays in sync with trial.ts constants', () => {
  // Drift guard: ensure CDK default matches trial.ts DEFAULT_TRIAL_DAYS
  const defaultTemplate = paymentTemplate();
  assert.equal(variables(defaultTemplate, 'jarvis-sandbox-stripe-checkout').TRIAL_DAYS, String(DEFAULT_TRIAL_DAYS));
  assert.equal(variables(defaultTemplate, 'jarvis-sandbox-me').TRIAL_DAYS, String(DEFAULT_TRIAL_DAYS));

  // MAX_TRIAL_DAYS should synthesise and reach both functions
  const maxTemplate = paymentTemplate({ trialDays: String(MAX_TRIAL_DAYS) });
  assert.equal(variables(maxTemplate, 'jarvis-sandbox-stripe-checkout').TRIAL_DAYS, String(MAX_TRIAL_DAYS));
  assert.equal(variables(maxTemplate, 'jarvis-sandbox-me').TRIAL_DAYS, String(MAX_TRIAL_DAYS));

  // Just over MAX_TRIAL_DAYS should throw
  assert.throws(() => paymentTemplate({ trialDays: String(MAX_TRIAL_DAYS + 1) }), /trialDays/);
});
