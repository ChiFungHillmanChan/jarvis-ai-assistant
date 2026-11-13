import test from 'node:test';
import assert from 'node:assert/strict';
import { registryTemplate } from '../../test-support/registry-template.cjs';

const of = (template: any, type: string) => Object.entries<any>(template.Resources).filter(([, r]) => r.Type === type);
const statements = (policy: any) => policy.Properties.PolicyDocument.Statement as any[];
const dev = registryTemplate('dev');

test('the registry bucket is private, encrypted, versioned and TLS-only', () => {
  const [[, bucket]] = of(dev, 'AWS::S3::Bucket');
  const p = bucket.Properties;
  assert.deepEqual(p.PublicAccessBlockConfiguration, { BlockPublicAcls: true, BlockPublicPolicy: true, IgnorePublicAcls: true, RestrictPublicBuckets: true });
  assert.equal(p.VersioningConfiguration.Status, 'Enabled');
  assert.equal(p.BucketEncryption.ServerSideEncryptionConfiguration[0].ServerSideEncryptionByDefault.SSEAlgorithm, 'AES256');
  assert.equal(p.OwnershipControls.Rules[0].ObjectOwnership, 'BucketOwnerEnforced');
  assert.equal(p.BucketName, 'jarvis-registry-dev-111111111111');
  const [[, bucketPolicy]] = of(dev, 'AWS::S3::BucketPolicy');
  assert.ok(statements(bucketPolicy).some(s => s.Effect === 'Deny' && s.Condition?.Bool?.['aws:SecureTransport'] === 'false'));
  assert.ok(!statements(bucketPolicy).some(s => s.Effect === 'Allow' && s.Principal === '*'), 'nothing is public on the bucket itself');
});

test('the prod bucket is kept when the stack is deleted', () => {
  const prod = registryTemplate('prod');
  const [[, bucket]] = of(prod, 'AWS::S3::Bucket');
  assert.equal(bucket.DeletionPolicy, 'Retain');
  assert.equal(of(prod, 'Custom::S3AutoDeleteObjects').length, 0);
});

test('only this repository\'s registry-<env> GitHub environment can assume the publisher role', () => {
  const role = of(dev, 'AWS::IAM::Role').map(([, r]) => r).find(r => r.Properties.RoleName === 'jarvis-dev-registry-publisher');
  assert.ok(role);
  const [trust] = role.Properties.AssumeRolePolicyDocument.Statement;
  assert.equal(trust.Action, 'sts:AssumeRoleWithWebIdentity');
  assert.deepEqual(trust.Condition, { StringEquals: {
    'token.actions.githubusercontent.com:aud': 'sts.amazonaws.com',
    'token.actions.githubusercontent.com:sub': 'repo:owner/repo:environment:registry-dev',
  } });
  assert.match(JSON.stringify(trust.Principal.Federated), /oidc-provider\/token\.actions\.githubusercontent\.com/);
  assert.equal(role.Properties.MaxSessionDuration, 3600);
});

test('the publisher may touch registry/* only, and the read Lambda latest.json only', () => {
  const policies = of(dev, 'AWS::IAM::Policy').map(([, p]) => p);
  const publisher = policies.find(p => JSON.stringify(p.Properties.Roles).includes('RegistryPublisherRole'));
  const actions = statements(publisher).flatMap(s => [].concat(s.Action));
  assert.deepEqual(actions.sort(), ['s3:GetObject', 's3:ListBucket', 's3:PutObject']);
  for (const s of statements(publisher)) {
    const text = JSON.stringify(s.Resource);
    if ([].concat(s.Action).includes('s3:ListBucket')) assert.deepEqual(s.Condition, { StringLike: { 's3:prefix': ['registry/*'] } });
    else assert.match(text, /\/registry\/\*"/);
    assert.ok(!/s3:DeleteObject|s3:\*|"\*"/.test(JSON.stringify(s)), 'no delete and no wildcard action');
  }
  const reader = policies.find(p => JSON.stringify(p.Properties.Roles).includes('RegistryReadFn'));
  assert.deepEqual(statements(reader).map(s => s.Action), ['s3:GetObject']);
  assert.match(JSON.stringify(statements(reader)[0].Resource), /\/registry\/latest\.json"/);
});

test('GET /v1/registry is a public route on the existing control-plane API', () => {
  const routes = of(dev, 'AWS::ApiGatewayV2::Route').map(([, r]) => r);
  assert.equal(routes.length, 1);
  assert.equal(routes[0].Properties.RouteKey, 'GET /v1/registry');
  assert.equal(routes[0].Properties.AuthorizationType, 'NONE');
  assert.equal(of(dev, 'AWS::ApiGatewayV2::Api').length, 0, 'no new API');
  assert.match(JSON.stringify(routes[0].Properties.ApiId), /"Fn::ImportValue":"Jarvis-dev-ApiBaseUrl"/);
  const [[, fn]] = of(dev, 'AWS::Lambda::Function').filter(([, f]) => f.Properties.FunctionName === 'jarvis-dev-registry-read');
  assert.equal(fn.Properties.Runtime, 'nodejs24.x');
  assert.equal(fn.Properties.Timeout, 5);
  assert.deepEqual(Object.keys(fn.Properties.Environment.Variables), ['REGISTRY_BUCKET']);
});
