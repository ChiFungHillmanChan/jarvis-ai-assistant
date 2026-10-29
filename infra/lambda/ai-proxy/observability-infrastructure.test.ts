import test from 'node:test';
import assert from 'node:assert/strict';
import { synthStack } from '../../test-support/stack-template.cjs';

const observability = (envName: string) => synthStack('lib/observability-stack.ts', 'ObservabilityStack', () => ({
  envName,
  billingAlertEmail: 'alerts@example.test',
  syncBucketName: 'fixture-sync-bucket',
}));

const ofType = (template: any, type: string): any[] =>
  Object.values(template.Resources).filter((r: any) => r.Type === type);

test('GuardDuty findings of medium severity or higher go to the alert topic', () => {
  for (const env of ['sandbox', 'prod']) {
    const template = observability(env);
    const rules = ofType(template, 'AWS::Events::Rule').filter((r: any) => r.Properties.Name === `jarvis-${env}-guardduty-findings`);
    assert.equal(rules.length, 1);
    const rule = rules[0].Properties;
    assert.equal(rule.State, 'ENABLED');
    assert.deepEqual(rule.EventPattern, {
      source: ['aws.guardduty'],
      'detail-type': ['GuardDuty Finding'],
      detail: { severity: [{ numeric: ['>=', 4] }] },
    });
    const topicId = Object.keys(template.Resources).find(k =>
      template.Resources[k].Type === 'AWS::SNS::Topic' && template.Resources[k].Properties.TopicName === `jarvis-${env}-alerts`);
    assert.equal(rule.Targets.length, 1);
    assert.deepEqual(rule.Targets[0].Arn, { Ref: topicId });
  }
});

test('EventBridge may publish to the alert topic', () => {
  const template = observability('sandbox');
  const policies = ofType(template, 'AWS::SNS::TopicPolicy');
  assert.equal(policies.length, 1);
  const publish = policies[0].Properties.PolicyDocument.Statement.filter((s: any) =>
    s.Principal?.Service === 'events.amazonaws.com' && s.Action === 'sns:Publish');
  assert.equal(publish.length, 1);
});
