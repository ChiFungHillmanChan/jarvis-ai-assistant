#!/usr/bin/env bash
# Run this ONCE from the AWS root account (or an admin user) to create
# a scoped CDK deployer IAM user for JARVIS infra.
#
# Why: the existing `jarvis` user is least-privilege (S3 + Bedrock only).
# Don't grant it admin. Create a dedicated deployer user with admin scope
# locked to the JARVIS account, used only by CDK.
#
# Required env: caller must be authenticated as admin (e.g. root or another
# admin profile on account 759626151650).
set -euo pipefail

USER_NAME="jarvis-cdk-deployer"
ACCOUNT_ID="759626151650"
PROFILE="${1:-default}"  # admin profile to use for this script

echo "Creating IAM user $USER_NAME on account $ACCOUNT_ID using profile $PROFILE"

aws iam create-user \
  --user-name "$USER_NAME" \
  --tags Key=Project,Value=jarvis Key=Purpose,Value=cdk-deploy \
  --profile "$PROFILE"

# Attach AdministratorAccess. CDK needs this to create roles, KMS keys, etc.
# Long-term: switch to CDK bootstrap-managed roles + permission boundary.
aws iam attach-user-policy \
  --user-name "$USER_NAME" \
  --policy-arn arn:aws:iam::aws:policy/AdministratorAccess \
  --profile "$PROFILE"

echo "Creating access key. Save this immediately, it's shown only once."
aws iam create-access-key --user-name "$USER_NAME" --profile "$PROFILE"

cat <<EOF

---
Done. To use this user locally:

  aws configure --profile jarvis-deploy
  # paste the access key + secret above
  # region: ap-southeast-1

Then verify:
  aws sts get-caller-identity --profile jarvis-deploy

CDK bootstrap (run once per region):
  cd infra
  npm install
  npx cdk bootstrap --profile jarvis-deploy aws://$ACCOUNT_ID/ap-southeast-1

---
EOF
