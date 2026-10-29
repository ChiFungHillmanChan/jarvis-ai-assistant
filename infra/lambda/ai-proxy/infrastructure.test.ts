import test from 'node:test';
import assert from 'node:assert/strict';
import { synthStack } from '../../test-support/stack-template.cjs';

const proxy = () => synthStack('lib/ai-proxy-stack.ts', 'AiProxyStack', (base: any, cdk: any) => ({
  envName: 'sandbox',
  billingAlertEmail: 'alerts@example.test',
  kmsKey: cdk.aws_kms.Key.fromKeyArn(base, 'Key', 'arn:aws:kms:ap-southeast-1:111111111111:key/fixture'),
  userPool: cdk.aws_cognito.UserPool.fromUserPoolId(base, 'Pool', 'ap-southeast-1_fixture'),
  userPoolClient: cdk.aws_cognito.UserPoolClient.fromUserPoolClientId(base, 'Client', 'fixture-client'),
  userMetaTable: cdk.aws_dynamodb.Table.fromTableName(base, 'Users', 'fixture-users'),
  aiTables: Object.fromEntries(['usage', 'period', 'grants', 'config'].map(k => [k, cdk.aws_dynamodb.Table.fromTableName(base, k, `fixture-ai-${k}`)])),
}));

const data = () => synthStack('lib/data-stack.ts', 'DataStack', (base: any, cdk: any) => ({
  envName: 'sandbox',
  kmsKey: cdk.aws_kms.Key.fromKeyArn(base, 'Key', 'arn:aws:kms:ap-southeast-1:111111111111:key/fixture'),
}));

const ofType = (template: any, type: string): any[] =>
  Object.values(template.Resources).filter((r: any) => r.Type === type);

const allowStatements = (template: any): any[] =>
  ofType(template, 'AWS::IAM::Policy')
    .flatMap((p: any) => p.Properties.PolicyDocument.Statement)
    .filter((s: any) => s.Effect === 'Allow');

const asList = (v: any): any[] => (Array.isArray(v) ? v : [v]);

test('the proxy streams through a Function URL with the spec limits', () => {
  const template = proxy();
  const fns = ofType(template, 'AWS::Lambda::Function').filter((f: any) => f.Properties.FunctionName === 'jarvis-sandbox-ai-proxy');
  assert.equal(fns.length, 1);
  const fn = fns[0].Properties;
  assert.equal(fn.Runtime, 'nodejs24.x');
  assert.equal(fn.Timeout, 300);
  assert.equal(fn.MemorySize, 512);
  assert.equal(fn.ReservedConcurrentExecutions, 50);
  const vars = fn.Environment.Variables;
  assert.equal(vars.USER_POOL_CLIENT_ID, 'fixture-client');
  assert.equal(vars.USER_POOL_ID, 'ap-southeast-1_fixture');
  assert.equal(vars.ENV, 'sandbox');
  assert.equal(vars.USER_META_TABLE, 'fixture-users');
  for (const [name, table] of [['AI_USAGE_TABLE', 'usage'], ['AI_PERIOD_TABLE', 'period'], ['AI_GRANTS_TABLE', 'grants'], ['AI_CONFIG_TABLE', 'config']]) {
    assert.equal(vars[name], `fixture-ai-${table}`, name);
  }
  assert.ok(vars.AI_LIMITS_TABLE, 'AI_LIMITS_TABLE is set');
  const urls = ofType(template, 'AWS::Lambda::Url');
  assert.equal(urls.length, 1);
  assert.equal(urls[0].Properties.AuthType, 'NONE');
  assert.equal(urls[0].Properties.InvokeMode, 'RESPONSE_STREAM');
  assert.equal(template.Outputs.AiProxyUrl.Export.Name, 'Jarvis-sandbox-AiProxyUrl');
});

test('the proxy role may invoke only the catalog models', () => {
  const template = proxy();
  const expected = [
    'arn:aws:bedrock:ap-northeast-1::foundation-model/qwen.qwen3-32b-v1:0',
    'arn:aws:bedrock:ap-northeast-1::foundation-model/deepseek.v3.2',
  ];
  const bedrock = allowStatements(template).filter((s: any) => asList(s.Action).some((a: string) => a.startsWith('bedrock:')));
  assert.ok(bedrock.length > 0, 'a bedrock statement exists');
  for (const s of bedrock) assert.deepEqual(asList(s.Resource), expected);
  assert.deepEqual(asList(bedrock[0].Action).sort(), ['bedrock:InvokeModel', 'bedrock:InvokeModelWithResponseStream']);
  const wildcard = allowStatements(template).filter((s: any) => asList(s.Resource).includes('*') && asList(s.Action).some((a: string) => a.startsWith('bedrock:')));
  assert.deepEqual(wildcard, []);
  const roles = ofType(template, 'AWS::IAM::Role').filter((r: any) => r.Properties.RoleName === 'jarvis-sandbox-ai-proxy');
  assert.equal(roles.length, 1);
});

test('limits table expires counters and proxy logs keep 30 days', () => {
  const template = proxy();
  const tables = ofType(template, 'AWS::DynamoDB::Table').filter((t: any) => t.Properties.TableName === 'jarvis-sandbox-ai-limits');
  assert.equal(tables.length, 1);
  const t = tables[0].Properties;
  assert.deepEqual(t.KeySchema, [{ AttributeName: 'pk', KeyType: 'HASH' }]);
  assert.equal(t.BillingMode, 'PAY_PER_REQUEST');
  assert.deepEqual(t.SSESpecification, { SSEEnabled: true, SSEType: 'KMS', KMSMasterKeyId: 'arn:aws:kms:ap-southeast-1:111111111111:key/fixture' });
  assert.deepEqual(t.TimeToLiveSpecification, { AttributeName: 'expiresAt', Enabled: true });
  assert.equal(t.PointInTimeRecoverySpecification.PointInTimeRecoveryEnabled, false);
  const logs = ofType(template, 'AWS::Logs::LogGroup').filter((l: any) => l.Properties.LogGroupName === '/aws/lambda/jarvis-sandbox-ai-proxy');
  assert.equal(logs.length, 1);
  assert.equal(logs[0].Properties.RetentionInDays, 30);
});

test('the AI tables are on-demand, KMS-encrypted and keyed as the spec says', () => {
  const template = data();
  const table = (name: string) => {
    const found = ofType(template, 'AWS::DynamoDB::Table').filter((t: any) => t.Properties.TableName === `jarvis-sandbox-ai-${name}`);
    assert.equal(found.length, 1, `jarvis-sandbox-ai-${name} exists once`);
    return found[0].Properties;
  };
  const keys: Record<string, any[]> = {
    usage: [{ AttributeName: 'userId', KeyType: 'HASH' }, { AttributeName: 'sk', KeyType: 'RANGE' }],
    period: [{ AttributeName: 'userId', KeyType: 'HASH' }, { AttributeName: 'periodKey', KeyType: 'RANGE' }],
    grants: [{ AttributeName: 'userId', KeyType: 'HASH' }, { AttributeName: 'grantId', KeyType: 'RANGE' }],
    config: [{ AttributeName: 'config', KeyType: 'HASH' }],
  };
  for (const [name, schema] of Object.entries(keys)) {
    const t = table(name);
    assert.deepEqual(t.KeySchema, schema, `${name} key schema`);
    assert.equal(t.BillingMode, 'PAY_PER_REQUEST', `${name} billing`);
    assert.equal(t.SSESpecification.SSEEnabled, true, `${name} SSE`);
    assert.equal(t.SSESpecification.SSEType, 'KMS', `${name} SSE type`);
    assert.equal(t.PointInTimeRecoverySpecification.PointInTimeRecoveryEnabled, false, `${name} PITR off outside prod`);
    for (const a of t.AttributeDefinitions) assert.equal(a.AttributeType, 'S', `${name}.${a.AttributeName} is a string`);
  }
  const usage = table('usage');
  assert.deepEqual(usage.TimeToLiveSpecification, { AttributeName: 'expiresAt', Enabled: true });
  assert.equal(usage.GlobalSecondaryIndexes.length, 1);
  const gsi = usage.GlobalSecondaryIndexes[0];
  assert.equal(gsi.IndexName, 'periodKey-index');
  assert.deepEqual(gsi.KeySchema, [{ AttributeName: 'periodKey', KeyType: 'HASH' }, { AttributeName: 'ts', KeyType: 'RANGE' }]);
  assert.deepEqual(gsi.Projection, { ProjectionType: 'ALL' });
});
