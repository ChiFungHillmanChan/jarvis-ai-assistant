#!/usr/bin/env bash
# JARVIS AWS cost monitor.
# Usage: ./aws-cost.sh [profile]
# Default profile: jarvis-deploy
#
# Shows:
#  - Month-to-date spend
#  - Per-service breakdown for current month
#  - Forecast for end of month
#  - Active resources count (DynamoDB, Lambda, S3 buckets, KMS keys)
set -euo pipefail

PROFILE="${1:-jarvis-deploy}"
ACCOUNT_ID=759626151650
REGION=ap-southeast-1

THIS_MONTH_START=$(date -u +%Y-%m-01)
TODAY=$(date -u +%Y-%m-%d)
NEXT_MONTH_START=$(date -u -v+1m +%Y-%m-01 2>/dev/null || date -u -d "$(date -u +%Y-%m-01) +1 month" +%Y-%m-01)

echo "=========================================="
echo "JARVIS AWS Cost Report"
echo "Account: $ACCOUNT_ID  Region: $REGION"
echo "Period: $THIS_MONTH_START to $TODAY"
echo "Profile: $PROFILE"
echo "=========================================="
echo ""

echo "[1/4] Month-to-date total ----------------"
aws ce get-cost-and-usage \
  --time-period "Start=$THIS_MONTH_START,End=$TODAY" \
  --granularity MONTHLY \
  --metrics BlendedCost UnblendedCost UsageQuantity \
  --profile "$PROFILE" \
  --query 'ResultsByTime[0].Total' --output table 2>&1

echo ""
echo "[2/4] Per-service breakdown --------------"
aws ce get-cost-and-usage \
  --time-period "Start=$THIS_MONTH_START,End=$TODAY" \
  --granularity MONTHLY \
  --metrics BlendedCost \
  --group-by Type=DIMENSION,Key=SERVICE \
  --profile "$PROFILE" \
  --query 'ResultsByTime[0].Groups[*].[Keys[0],Metrics.BlendedCost.Amount]' \
  --output table 2>&1

echo ""
echo "[3/4] End-of-month forecast --------------"
aws ce get-cost-forecast \
  --time-period "Start=$TODAY,End=$NEXT_MONTH_START" \
  --metric BLENDED_COST \
  --granularity MONTHLY \
  --profile "$PROFILE" \
  --query '[Total.Amount,Total.Unit]' --output text 2>&1 || echo "(forecast not available -- need 5+ days of data)"

echo ""
echo "[4/4] Active resources -------------------"
echo -n "  Lambda functions: "
aws lambda list-functions --profile "$PROFILE" --region "$REGION" --query 'length(Functions[?starts_with(FunctionName, `jarvis-`)])' --output text 2>&1
echo -n "  DynamoDB tables:  "
aws dynamodb list-tables --profile "$PROFILE" --region "$REGION" --query 'length(TableNames[?starts_with(@, `jarvis-`)])' --output text 2>&1
echo -n "  S3 buckets:       "
aws s3api list-buckets --profile "$PROFILE" --query 'length(Buckets[?starts_with(Name, `jarvis-`)])' --output text 2>&1
echo -n "  KMS keys (alias): "
aws kms list-aliases --profile "$PROFILE" --region "$REGION" --query 'length(Aliases[?starts_with(AliasName, `alias/jarvis/`)])' --output text 2>&1
echo -n "  Cognito pools:    "
aws cognito-idp list-user-pools --max-results 60 --profile "$PROFILE" --region "$REGION" --query 'length(UserPools[?starts_with(Name, `jarvis-`)])' --output text 2>&1
echo -n "  Active CFN stacks: "
aws cloudformation list-stacks --profile "$PROFILE" --region "$REGION" --stack-status-filter CREATE_COMPLETE UPDATE_COMPLETE --query 'length(StackSummaries[?starts_with(StackName, `Jarvis-`)])' --output text 2>&1

echo ""
echo "=========================================="
echo "Budget thresholds (sandbox): \$5 / \$10 / \$25"
echo "Alert email: hillmanchan709@gmail.com"
echo "=========================================="
