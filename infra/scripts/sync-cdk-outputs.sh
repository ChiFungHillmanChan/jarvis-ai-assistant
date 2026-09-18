#!/usr/bin/env bash
# Sync CDK stack outputs into a Tauri-readable env file.
# Run after `npm run deploy:<env>`.
set -euo pipefail

ENV="${1:-sandbox}"
OUT_DIR="cdk.out"
TARGET="../src-tauri/.env.${ENV}.aws"

if [[ ! -d "$OUT_DIR" ]]; then
  echo "No $OUT_DIR -- run cdk deploy first." >&2
  exit 1
fi

echo "# Auto-generated from CDK outputs. Do not edit." > "$TARGET"
echo "# Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$TARGET"
echo "# Env: $ENV" >> "$TARGET"
echo "" >> "$TARGET"

# Look up the relevant outputs by stack name
USER_POOL_ID=$(aws cloudformation describe-stacks \
  --stack-name "Jarvis-${ENV}-Auth" \
  --query "Stacks[0].Outputs[?ExportName=='Jarvis-${ENV}-UserPoolId'].OutputValue" \
  --output text --profile jarvis-deploy --region ap-southeast-1 || echo "")
CLIENT_ID=$(aws cloudformation describe-stacks \
  --stack-name "Jarvis-${ENV}-Auth" \
  --query "Stacks[0].Outputs[?ExportName=='Jarvis-${ENV}-UserPoolClientId'].OutputValue" \
  --output text --profile jarvis-deploy --region ap-southeast-1 || echo "")
KEY_ARN=$(aws cloudformation describe-stacks \
  --stack-name "Jarvis-${ENV}-Auth" \
  --query "Stacks[0].Outputs[?ExportName=='Jarvis-${ENV}-DataKeyArn'].OutputValue" \
  --output text --profile jarvis-deploy --region ap-southeast-1 || echo "")
SYNC_BUCKET=$(aws cloudformation describe-stacks \
  --stack-name "Jarvis-${ENV}-Data" \
  --query "Stacks[0].Outputs[?ExportName=='Jarvis-${ENV}-SyncBucketName'].OutputValue" \
  --output text --profile jarvis-deploy --region ap-southeast-1 || echo "")
COGNITO_DOMAIN=$(aws cloudformation describe-stacks \
  --stack-name "Jarvis-${ENV}-Auth" \
  --query "Stacks[0].Outputs[?ExportName=='Jarvis-${ENV}-UserPoolDomainUrl'].OutputValue" \
  --output text --profile jarvis-deploy --region ap-southeast-1 || echo "")
API_BASE=$(aws cloudformation describe-stacks \
  --stack-name "Jarvis-${ENV}-Payment" \
  --query "Stacks[0].Outputs[?ExportName=='Jarvis-${ENV}-ApiBaseUrl'].OutputValue" \
  --output text --profile jarvis-deploy --region ap-southeast-1 || echo "")

cat >> "$TARGET" <<EOF
JARVIS_AWS_REGION=ap-southeast-1
JARVIS_COGNITO_USER_POOL_ID=$USER_POOL_ID
JARVIS_COGNITO_CLIENT_ID=$CLIENT_ID
JARVIS_COGNITO_DOMAIN=$COGNITO_DOMAIN
JARVIS_PAYMENT_API=$API_BASE
JARVIS_KMS_KEY_ARN=$KEY_ARN
JARVIS_SYNC_BUCKET=$SYNC_BUCKET
EOF

echo "Wrote $TARGET"
