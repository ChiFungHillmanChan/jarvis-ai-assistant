import test from 'node:test';
import assert from 'node:assert/strict';
import { paymentTemplate } from '../../test-support/payment-template.cjs';

test('the API stage waits for every route named in RouteSettings before updating throttles', () => {
  const template = paymentTemplate();
  const resources = Object.entries(template.Resources);
  const stage = resources.find(([, resource]: [string, any]) => resource.Type === 'AWS::ApiGatewayV2::Stage')?.[1] as any;
  assert.ok(stage, 'Payment API has a default stage');
  for (const routeKey of Object.keys(stage.Properties.RouteSettings)) {
    const route = resources.find(([, resource]: [string, any]) => resource.Type === 'AWS::ApiGatewayV2::Route' && resource.Properties.RouteKey === routeKey);
    assert.ok(route, `RouteSettings refers to a declared route: ${routeKey}`);
    assert.ok(stage.DependsOn?.includes(route[0]), `Stage must wait for ${routeKey}; API Gateway rejects throttles for a route that does not yet exist`);
  }
});
