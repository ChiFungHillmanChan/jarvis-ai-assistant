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

test('the GuardDuty rule publishes through a role, not a topic policy open to all of EventBridge', () => {
  const template = observability('sandbox');
  // A topic policy for events.amazonaws.com carries no aws:SourceArn, so any rule in any account could use it.
  assert.equal(ofType(template, 'AWS::SNS::TopicPolicy').length, 0);
  const rule = ofType(template, 'AWS::Events::Rule')[0].Properties;
  assert.equal(rule.Targets.length, 1);
  const roleId = rule.Targets[0].RoleArn['Fn::GetAtt'][0];
  const role = template.Resources[roleId];
  assert.deepEqual(role.Properties.AssumeRolePolicyDocument.Statement[0].Principal, { Service: 'events.amazonaws.com' });
  const topicId = Object.keys(template.Resources).find(k =>
    template.Resources[k].Type === 'AWS::SNS::Topic' && template.Resources[k].Properties.TopicName === 'jarvis-sandbox-alerts');
  const publish = ofType(template, 'AWS::IAM::Policy')
    .filter((p: any) => p.Properties.Roles.some((r: any) => r.Ref === roleId))
    .flatMap((p: any) => p.Properties.PolicyDocument.Statement);
  assert.equal(publish.length, 1);
  assert.equal(publish[0].Action, 'sns:Publish');
  assert.equal(publish[0].Effect, 'Allow');
  assert.deepEqual(publish[0].Resource, { Ref: topicId });
});
