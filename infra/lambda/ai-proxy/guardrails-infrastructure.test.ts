import test from 'node:test';
import assert from 'node:assert/strict';
import { synthStack } from '../../test-support/stack-template.cjs';

const proxy = (envName: string) => synthStack('lib/ai-proxy-stack.ts', 'AiProxyStack', (base: any, cdk: any) => ({
  envName,
  billingAlertEmail: 'alerts@example.test',
  kmsKey: cdk.aws_kms.Key.fromKeyArn(base, 'Key', 'arn:aws:kms:ap-southeast-1:111111111111:key/fixture'),
  userPool: cdk.aws_cognito.UserPool.fromUserPoolId(base, 'Pool', 'ap-southeast-1_fixture'),
  userPoolClient: cdk.aws_cognito.UserPoolClient.fromUserPoolClientId(base, 'Client', 'fixture-client'),
  userMetaTable: cdk.aws_dynamodb.Table.fromTableName(base, 'Users', 'fixture-users'),
  aiTables: Object.fromEntries(['usage', 'period', 'grants', 'config'].map(k => [k, cdk.aws_dynamodb.Table.fromTableName(base, k, `fixture-ai-${k}`)])),
}));

const ofType = (template: any, type: string): any[] =>
  Object.values(template.Resources).filter((r: any) => r.Type === type);

const asList = (v: any): any[] => (Array.isArray(v) ? v : [v]);

const proxyRoleLogicalId = (template: any): string => {
  const found = Object.entries(template.Resources).filter(([, r]: any) =>
    r.Type === 'AWS::IAM::Role' && r.Properties.RoleName === 'jarvis-sandbox-ai-proxy');
  assert.equal(found.length, 1);
  return found[0][0];
};

test('sandbox owns the Bedrock-only budget with alerts at 50, 80 and 100 percent', () => {
  const budgets = ofType(proxy('sandbox'), 'AWS::Budgets::Budget');
  assert.equal(budgets.length, 1);
  const { Budget, NotificationsWithSubscribers } = budgets[0].Properties;
  assert.equal(Budget.BudgetName, 'jarvis-bedrock-monthly');
  assert.equal(Budget.BudgetType, 'COST');
  assert.equal(Budget.TimeUnit, 'MONTHLY');
  assert.deepEqual(Budget.BudgetLimit, { Amount: 100, Unit: 'USD' });
  assert.deepEqual(Budget.CostFilters, { Service: ['Amazon Bedrock'] });
  // Credits and refunds would hide real Bedrock spend, so the alerts and the kill switch count only real cost.
  assert.equal(Budget.CostTypes.IncludeCredit, false);
  assert.equal(Budget.CostTypes.IncludeRefund, false);
  assert.deepEqual(NotificationsWithSubscribers.map((n: any) => n.Notification.Threshold), [50, 80, 100]);
  for (const n of NotificationsWithSubscribers) {
    assert.equal(n.Notification.NotificationType, 'ACTUAL');
    assert.equal(n.Notification.ThresholdType, 'PERCENTAGE');
    assert.equal(n.Notification.ComparisonOperator, 'GREATER_THAN');
    assert.deepEqual(n.Subscribers, [{ SubscriptionType: 'EMAIL', Address: 'alerts@example.test' }]);
  }
});

test('the 150 percent action applies the kill switch to the proxy role automatically', () => {
  const template = proxy('sandbox');
  const actions = ofType(template, 'AWS::Budgets::BudgetsAction');
  assert.equal(actions.length, 1);
  const a = actions[0].Properties;
  assert.equal(a.NotificationType, 'ACTUAL');
  assert.equal(a.ActionType, 'APPLY_IAM_POLICY');
  assert.deepEqual(a.ActionThreshold, { Type: 'PERCENTAGE', Value: 150 });
  assert.equal(a.ApprovalModel, 'AUTOMATIC');
  assert.deepEqual(a.Subscribers, [{ Type: 'EMAIL', Address: 'alerts@example.test' }]);
  assert.deepEqual(a.Definition.IamActionDefinition.Roles, [{ Ref: proxyRoleLogicalId(template) }]);
  const policies = ofType(template, 'AWS::IAM::ManagedPolicy').filter((p: any) => p.Properties.ManagedPolicyName === 'jarvis-bedrock-kill-switch');
  assert.equal(policies.length, 1);
  const killSwitchId = Object.keys(template.Resources).find(k => template.Resources[k] === policies[0]);
  assert.deepEqual(a.Definition.IamActionDefinition.PolicyArn, { Ref: killSwitchId });

  // The action hangs off the budget it names and cannot be created before it.
  const budgetId = Object.keys(template.Resources).find(k => template.Resources[k].Type === 'AWS::Budgets::Budget');
  assert.deepEqual(a.BudgetName, 'jarvis-bedrock-monthly');
  assert.ok(asList(actions[0].DependsOn).includes(budgetId), 'action depends on the budget');

  // Budgets may attach and detach only the kill switch, only on the proxy role.
  const roles = ofType(template, 'AWS::IAM::Role').filter((r: any) =>
    JSON.stringify(r.Properties.AssumeRolePolicyDocument).includes('budgets.amazonaws.com'));
  assert.equal(roles.length, 1);
  assert.equal(a.ExecutionRoleArn['Fn::GetAtt'][0], Object.keys(template.Resources).find(k => template.Resources[k] === roles[0]));
  const budgetRoleId = a.ExecutionRoleArn['Fn::GetAtt'][0];
  const defaultPolicyId = Object.keys(template.Resources).find(k =>
    template.Resources[k].Type === 'AWS::IAM::Policy' && template.Resources[k].Properties.Roles.some((r: any) => r.Ref === budgetRoleId));
  assert.ok(defaultPolicyId, 'the budgets role has a policy');
  assert.ok(asList(actions[0].DependsOn).includes(defaultPolicyId), 'action waits for the role policy');
  const attached = ofType(template, 'AWS::IAM::Policy').filter((p: any) =>
    p.Properties.Roles.some((r: any) => r.Ref === a.ExecutionRoleArn['Fn::GetAtt'][0]));
  const stmts = attached.flatMap((p: any) => p.Properties.PolicyDocument.Statement);
  assert.equal(stmts.length, 1);
  assert.deepEqual(asList(stmts[0].Action).sort(), ['iam:AttachRolePolicy', 'iam:DetachRolePolicy']);
  assert.deepEqual(stmts[0].Resource, { 'Fn::GetAtt': [proxyRoleLogicalId(template), 'Arn'] });
  assert.deepEqual(stmts[0].Condition, { ArnEquals: { 'iam:PolicyARN': { Ref: killSwitchId } } });
});

test('the kill switch denies all Bedrock invocation and is not attached', () => {
  const template = proxy('sandbox');
  const policies = ofType(template, 'AWS::IAM::ManagedPolicy').filter((p: any) => p.Properties.ManagedPolicyName === 'jarvis-bedrock-kill-switch');
  assert.equal(policies.length, 1);
  const p = policies[0].Properties;
  assert.deepEqual(p.PolicyDocument.Statement.length, 1);
  const s = p.PolicyDocument.Statement[0];
  assert.equal(s.Effect, 'Deny');
  assert.equal(s.Action, 'bedrock:InvokeModel*');
  assert.equal(s.Resource, '*');
  for (const k of ['Roles', 'Users', 'Groups']) assert.equal(p[k], undefined, `${k} is not set`);
});

test('other envs get no budget or action, but keep the per-user alarm', () => {
  const template = proxy('dev');
  assert.equal(ofType(template, 'AWS::Budgets::Budget').length, 0);
  assert.equal(ofType(template, 'AWS::Budgets::BudgetsAction').length, 0);
  assert.equal(ofType(template, 'AWS::IAM::ManagedPolicy').length, 0);
  const alarms = ofType(template, 'AWS::CloudWatch::Alarm').filter((a: any) => a.Properties.AlarmName === 'jarvis-dev-ai-user-daily-over-20usd');
  assert.equal(alarms.length, 1);
  assert.equal(alarms[0].Properties.MetricName, 'UserDailyBilledOver20');
  assert.deepEqual(alarms[0].Properties.Dimensions, [{ Name: 'Env', Value: 'dev' }]);
});

test('every env alarms on the per-user daily metric and notifies the AI alerts topic', () => {
  for (const env of ['sandbox', 'dev']) {
    const template = proxy(env);
    const topics = ofType(template, 'AWS::SNS::Topic').filter((t: any) => t.Properties.TopicName === `jarvis-${env}-ai-alerts`);
    assert.equal(topics.length, 1);
    const topicId = Object.keys(template.Resources).find(k => template.Resources[k] === topics[0]);
    const subs = ofType(template, 'AWS::SNS::Subscription');
    assert.equal(subs.length, 1);
    assert.equal(subs[0].Properties.Protocol, 'email');
    assert.equal(subs[0].Properties.Endpoint, 'alerts@example.test');
    assert.deepEqual(subs[0].Properties.TopicArn, { Ref: topicId });
    const daily = ofType(template, 'AWS::CloudWatch::Alarm').filter((x: any) => x.Properties.AlarmName === `jarvis-${env}-ai-user-daily-over-20usd`);
    assert.equal(daily.length, 1);
    const a = daily[0].Properties;
    assert.equal(a.Namespace, 'Jarvis/AiProxy');
    assert.equal(a.MetricName, 'UserDailyBilledOver20');
    assert.deepEqual(a.Dimensions, [{ Name: 'Env', Value: env }]);
    assert.equal(a.Statistic, 'Sum');
    assert.equal(a.Period, 300);
    assert.equal(a.EvaluationPeriods, 1);
    assert.equal(a.Threshold, 1);
    assert.equal(a.ComparisonOperator, 'GreaterThanOrEqualToThreshold');
    assert.equal(a.TreatMissingData, 'notBreaching');
    assert.deepEqual(a.AlarmActions, [{ Ref: topicId }]);
  }
});

test('every env alarms when a served request could not be metered', () => {
  for (const env of ['sandbox', 'dev']) {
    const template = proxy(env);
    const idOf = (resource: any) => Object.keys(template.Resources).find(k => template.Resources[k] === resource);
    const logGroups = ofType(template, 'AWS::Logs::LogGroup').filter((l: any) => l.Properties.LogGroupName === `/aws/lambda/jarvis-${env}-ai-proxy`);
    assert.equal(logGroups.length, 1);
    const filters = ofType(template, 'AWS::Logs::MetricFilter');
    assert.equal(filters.length, 1);
    const f = filters[0].Properties;
    assert.deepEqual(f.LogGroupName, { Ref: idOf(logGroups[0]) });
    assert.equal(f.FilterPattern, '{ ($.event = "converse") && ($.meterFailed IS TRUE) }');
    // The converse log has no env field, so the env is in the metric name; the filter reads only this env's log group.
    assert.deepEqual(f.MetricTransformations, [{ MetricNamespace: 'Jarvis/AiProxy', MetricName: `MeterFailed-${env}`, MetricValue: '1', Unit: 'Count' }]);

    const topic = ofType(template, 'AWS::SNS::Topic').find((t: any) => t.Properties.TopicName === `jarvis-${env}-ai-alerts`);
    const alarms = ofType(template, 'AWS::CloudWatch::Alarm').filter((a: any) => a.Properties.AlarmName === `jarvis-${env}-ai-meter-failed`);
    assert.equal(alarms.length, 1);
    const a = alarms[0].Properties;
    assert.equal(a.Namespace, 'Jarvis/AiProxy');
    assert.equal(a.MetricName, `MeterFailed-${env}`);
    assert.equal(a.Dimensions, undefined);
    assert.equal(a.Statistic, 'Sum');
    assert.equal(a.Period, 300);
    assert.equal(a.EvaluationPeriods, 1);
    assert.equal(a.Threshold, 1);
    assert.equal(a.ComparisonOperator, 'GreaterThanOrEqualToThreshold');
    assert.equal(a.TreatMissingData, 'notBreaching');
    assert.deepEqual(a.AlarmActions, [{ Ref: idOf(topic) }]);
  }
});

test('every env alarms when the real-time global daily ceiling is reached', () => {
  for (const env of ['sandbox', 'dev']) {
    const template = proxy(env);
    const topic = ofType(template, 'AWS::SNS::Topic').find((t: any) => t.Properties.TopicName === `jarvis-${env}-ai-alerts`);
    const topicId = Object.keys(template.Resources).find(k => template.Resources[k] === topic);
    const alarms = ofType(template, 'AWS::CloudWatch::Alarm').filter((x: any) => x.Properties.AlarmName === `jarvis-${env}-ai-global-daily-ceiling`);
    assert.equal(alarms.length, 1);
    const a = alarms[0].Properties;
    assert.equal(a.Namespace, 'Jarvis/AiProxy');
    assert.equal(a.MetricName, 'GlobalDailyCeilingReached');
    assert.deepEqual(a.Dimensions, [{ Name: 'Env', Value: env }]);
    assert.equal(a.Statistic, 'Sum');
    assert.equal(a.Period, 300);
    assert.equal(a.EvaluationPeriods, 1);
    assert.equal(a.Threshold, 1);
    assert.equal(a.ComparisonOperator, 'GreaterThanOrEqualToThreshold');
    assert.equal(a.TreatMissingData, 'notBreaching');
    assert.deepEqual(a.AlarmActions, [{ Ref: topicId }]);
  }
});

test('the proxy role still has no wildcard Bedrock Allow', () => {
  const template = proxy('sandbox');
  const bedrockAllows = ofType(template, 'AWS::IAM::Policy')
    .flatMap((p: any) => p.Properties.PolicyDocument.Statement)
    .filter((s: any) => s.Effect === 'Allow' && asList(s.Action).some((a: string) => a.startsWith('bedrock:')));
  assert.ok(bedrockAllows.length > 0);
  for (const s of bedrockAllows) assert.ok(!asList(s.Resource).includes('*'), 'no wildcard resource');
  // The Deny lives only in the unattached kill-switch policy.
  const attachedDeny = ofType(template, 'AWS::IAM::Policy')
    .flatMap((p: any) => p.Properties.PolicyDocument.Statement)
    .filter((s: any) => s.Effect === 'Deny');
  assert.deepEqual(attachedDeny, []);
});
