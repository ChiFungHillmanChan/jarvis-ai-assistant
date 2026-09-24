import type { APIGatewayProxyEventV2WithJWTAuthorizer, APIGatewayProxyResultV2 } from 'aws-lambda';
import { readLocale } from '../stripe-checkout/body';
import { failure, json, loadBilling, portalSession, userRow, verifyCustomer } from '../stripe-checkout/billing';

export async function handler(event: APIGatewayProxyEventV2WithJWTAuthorizer): Promise<APIGatewayProxyResultV2> {
  const sub = event.requestContext.authorizer?.jwt?.claims?.sub;
  if (typeof sub !== 'string' || !sub) return json(401, { error: 'missing_identity' });
  try {
    const row = await userRow(sub);
    if (!row.stripeCustomerId) return json(404, { error: 'billing_customer_missing' });
    const { stripe } = await loadBilling();
    await verifyCustomer(stripe, row.stripeCustomerId, sub);
    const session = await portalSession(stripe, row.stripeCustomerId, readLocale(event.body, event.isBase64Encoded));
    return json(200, { url: session.url, session_id: session.id });
  } catch (error) { return failure(error); }
}
