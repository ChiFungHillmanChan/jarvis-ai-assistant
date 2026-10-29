import test from 'node:test';
import assert from 'node:assert/strict';
import { paymentTemplate } from '../../test-support/payment-template.cjs';

const ofType = (template: any, type: string): any[] =>
  Object.values(template.Resources).filter((r: any) => r.Type === type);

test('POST /me/ai-access-request needs a Cognito access token and is throttled', () => {
  const template = paymentTemplate();
  const routes = ofType(template, 'AWS::ApiGatewayV2::Route').filter((r: any) => r.Properties.RouteKey === 'POST /me/ai-access-request');
  assert.equal(routes.length, 1);
  assert.equal(routes[0].Properties.AuthorizationType, 'JWT');
  assert.deepEqual(routes[0].Properties.AuthorizationScopes, ['openid']);
  const stage = ofType(template, 'AWS::ApiGatewayV2::Stage')[0];
  assert.deepEqual(stage.Properties.RouteSettings['POST /me/ai-access-request'], { ThrottlingRateLimit: 2, ThrottlingBurstLimit: 5 });
});

test('the me function reads the AI tables and may write user-meta', () => {
  const template = paymentTemplate();
  const me = ofType(template, 'AWS::Lambda::Function').find((f: any) => f.Properties.FunctionName === 'jarvis-sandbox-me');
  assert.ok(me, 'me function exists');
  const vars = me.Properties.Environment.Variables;
  assert.equal(vars.AI_GRANTS_TABLE, 'fixture-ai-grants');
  assert.equal(vars.AI_PERIOD_TABLE, 'fixture-ai-period');
  assert.equal(vars.AI_CONFIG_TABLE, 'fixture-ai-config');
  const role = me.Properties.Role['Fn::GetAtt'][0];
  const policy = ofType(template, 'AWS::IAM::Policy').find((p: any) => p.Properties.Roles.some((r: any) => r.Ref === role));
  assert.ok(policy, 'me role has a policy');
  const actions = policy.Properties.PolicyDocument.Statement.flatMap((s: any) => (Array.isArray(s.Action) ? s.Action : [s.Action]));
  assert.ok(actions.includes('dynamodb:UpdateItem'));
});

test('the access-request function exists and may write user-meta', () => {
  const template = paymentTemplate();
  const fn = ofType(template, 'AWS::Lambda::Function').find((f: any) => f.Properties.FunctionName === 'jarvis-sandbox-ai-access-request');
  assert.ok(fn, 'access-request function exists');
  assert.equal(fn.Properties.Environment.Variables.USER_META_TABLE, 'fixture-users');
  const role = fn.Properties.Role['Fn::GetAtt'][0];
  const policy = ofType(template, 'AWS::IAM::Policy').find((p: any) => p.Properties.Roles.some((r: any) => r.Ref === role));
  const actions = policy.Properties.PolicyDocument.Statement.flatMap((s: any) => (Array.isArray(s.Action) ? s.Action : [s.Action]));
  assert.ok(actions.includes('dynamodb:UpdateItem'));
  assert.ok(actions.includes('dynamodb:GetItem'));
});
