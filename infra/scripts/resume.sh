#!/usr/bin/env bash
# Resume JARVIS sandbox after pause.
# Re-deploys all 5 CDK stacks, re-registers Stripe webhook, refreshes Secrets Manager.
# Idempotent -- safe to re-run.
#
# Usage: ./resume.sh
set -euo pipefail

PROFILE=jarvis-deploy
REGION=ap-southeast-1

echo "=========================================="
echo "JARVIS AWS sandbox -- RESUME"
echo "=========================================="
echo ""

cd "$(dirname "$0")/.."

echo "[1/6] Deploy all CDK stacks..."
CDK_DEFAULT_ACCOUNT=759626151650 CDK_DEFAULT_REGION=$REGION \
  npx cdk deploy --all --profile $PROFILE --context env=sandbox --require-approval never

echo ""
echo "[2/6] Read CDK outputs..."
WEBHOOK_URL=$(aws cloudformation describe-stacks \
  --stack-name Jarvis-sandbox-Payment \
  --profile $PROFILE --region $REGION \
  --query 'Stacks[0].Outputs[?OutputKey==`StripeWebhookUrl`].OutputValue' --output text)
CHECKOUT_URL=$(aws cloudformation describe-stacks \
  --stack-name Jarvis-sandbox-Payment \
  --profile $PROFILE --region $REGION \
  --query 'Stacks[0].Outputs[?OutputKey==`CheckoutUrl`].OutputValue' --output text)
echo "  Webhook:  $WEBHOOK_URL"
echo "  Checkout: $CHECKOUT_URL"

echo ""
echo "[3/6] Register Stripe webhook (delete any existing first)..."
EXISTING=$(stripe webhook_endpoints list --limit 100 2>/dev/null | python3 -c "
import sys, json
data = json.load(sys.stdin)
for w in data.get('data', []):
    if 'jarvis' in (w.get('description') or '').lower() or 'execute-api' in w.get('url', ''):
        print(w['id'])
")
for w in $EXISTING; do
  stripe webhook_endpoints delete $w --confirm >/dev/null 2>&1 || true
done

WEBHOOK=$(stripe webhook_endpoints create \
  --url=$WEBHOOK_URL \
  --description="JARVIS sandbox" \
  -d "enabled_events[]=checkout.session.completed" \
  -d "enabled_events[]=customer.subscription.created" \
  -d "enabled_events[]=customer.subscription.updated" \
  -d "enabled_events[]=customer.subscription.deleted" \
  -d "enabled_events[]=invoice.payment_failed")
WHSEC=$(echo "$WEBHOOK" | python3 -c "import sys,json;print(json.load(sys.stdin)['secret'])")
WID=$(echo "$WEBHOOK" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
echo "  Webhook ID: $WID"

echo ""
echo "[4/6] Read Stripe API key..."
API_KEY=$(stripe config --list 2>&1 | awk '/^\[default\]/{flag=1} flag && /test_mode_api_key/{print $3; exit}' | tr -d "'")
echo "  API key prefix: $(echo $API_KEY | cut -c1-12)..."

echo ""
echo "[5/6] Update Secrets Manager..."
aws secretsmanager update-secret \
  --secret-id jarvis/sandbox/stripe \
  --secret-string "{\"webhookSecret\":\"$WHSEC\",\"apiKey\":\"$API_KEY\",\"webhookEndpointId\":\"$WID\",\"priceProMonthlyHkd\":\"price_1TUsAuJCG4qg3rNcZVpOopcD\",\"priceTeamMonthlyHkd\":\"price_1TUsAwJCG4qg3rNcPY3YJiOB\"}" \
  --profile $PROFILE --region $REGION --query VersionId --output text

echo ""
echo "[6/6] Force Lambda cold start..."
TS=$(date +%s)
aws lambda update-function-configuration \
  --function-name jarvis-sandbox-stripe-webhook \
  --environment "Variables={STRIPE_SECRET_NAME=jarvis/sandbox/stripe,USER_META_TABLE=jarvis-sandbox-user-meta,ENV=sandbox,SECRET_VERSION=$TS}" \
  --profile $PROFILE --region $REGION --query LastModified --output text >/dev/null
aws lambda update-function-configuration \
  --function-name jarvis-sandbox-stripe-checkout \
  --environment "Variables={STRIPE_SECRET_NAME=jarvis/sandbox/stripe,USER_META_TABLE=jarvis-sandbox-user-meta,ENV=sandbox,APP_DOMAIN=jarvis-automation.com,SECRET_VERSION=$TS}" \
  --profile $PROFILE --region $REGION --query LastModified --output text >/dev/null

bash "$(dirname "$0")/sync-cdk-outputs.sh" sandbox

echo ""
echo "=========================================="
echo "RESUMED. Verify with:"
echo "  curl -X POST $CHECKOUT_URL -H 'Content-Type: application/json' \\"
echo "    -d '{\"tier\":\"pro\",\"user_id\":\"resume-test\",\"email\":\"hillmanchan709@gmail.com\"}'"
echo ""
echo "Don't forget: re-confirm SNS email subscription if you receive one."
echo "=========================================="
