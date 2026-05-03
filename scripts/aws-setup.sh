#!/usr/bin/env bash
# Idempotent AWS provisioning for JARVIS releases.
# Usage: ./scripts/aws-setup.sh [bucket-name]
# Default bucket: jarvis-releases (or jarvis-releases-dev for dev pipeline).

set -euo pipefail

BUCKET="${1:-jarvis-releases}"
REGION="ap-east-1"
PROFILE="${AWS_PROFILE:-jarvis}"
IAM_USER="jarvis-release-bot"

echo "==> Configuring bucket s3://${BUCKET} in ${REGION} (profile: ${PROFILE})"

# 1. Bucket
if aws s3api head-bucket --bucket "${BUCKET}" --profile "${PROFILE}" 2>/dev/null; then
  echo "Bucket exists, skipping create."
else
  aws s3api create-bucket \
    --bucket "${BUCKET}" \
    --region "${REGION}" \
    --create-bucket-configuration LocationConstraint="${REGION}" \
    --profile "${PROFILE}"
fi

# 2. Block public ACLs (we use bucket policy, not ACLs)
aws s3api put-public-access-block \
  --bucket "${BUCKET}" \
  --public-access-block-configuration BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=false,RestrictPublicBuckets=false \
  --profile "${PROFILE}"

# 3. Public-read bucket policy on .dmg / .sig / latest.json
cat > /tmp/jarvis-bucket-policy.json <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "PublicReadReleases",
      "Effect": "Allow",
      "Principal": "*",
      "Action": "s3:GetObject",
      "Resource": [
        "arn:aws:s3:::${BUCKET}/*.dmg",
        "arn:aws:s3:::${BUCKET}/*.dmg.sig",
        "arn:aws:s3:::${BUCKET}/latest.json"
      ]
    }
  ]
}
POLICY
aws s3api put-bucket-policy \
  --bucket "${BUCKET}" \
  --policy file:///tmp/jarvis-bucket-policy.json \
  --profile "${PROFILE}"

# 4. CORS - allow the website (and localhost) to fetch latest.json client-side
cat > /tmp/jarvis-cors.json <<CORS
{
  "CORSRules": [
    {
      "AllowedMethods": ["GET", "HEAD"],
      "AllowedOrigins": [
        "https://jarvis-ai.com",
        "https://www.jarvis-ai.com",
        "http://localhost:3000"
      ],
      "AllowedHeaders": ["*"],
      "ExposeHeaders": ["ETag"]
    }
  ]
}
CORS
aws s3api put-bucket-cors \
  --bucket "${BUCKET}" \
  --cors-configuration file:///tmp/jarvis-cors.json \
  --profile "${PROFILE}"

# 5. Lifecycle - keep 5 latest versions, delete >90 day old non-latest
aws s3api put-bucket-versioning \
  --bucket "${BUCKET}" \
  --versioning-configuration Status=Enabled \
  --profile "${PROFILE}"

cat > /tmp/jarvis-lifecycle.json <<LIFE
{
  "Rules": [
    {
      "ID": "expire-old-versions",
      "Status": "Enabled",
      "Filter": {},
      "NoncurrentVersionExpiration": { "NoncurrentDays": 90 }
    }
  ]
}
LIFE
aws s3api put-bucket-lifecycle-configuration \
  --bucket "${BUCKET}" \
  --lifecycle-configuration file:///tmp/jarvis-lifecycle.json \
  --profile "${PROFILE}"

# 6. IAM user with PutObject only on this bucket
if aws iam get-user --user-name "${IAM_USER}" --profile "${PROFILE}" 2>/dev/null; then
  echo "IAM user ${IAM_USER} exists, skipping create."
else
  aws iam create-user --user-name "${IAM_USER}" --profile "${PROFILE}"
fi

cat > /tmp/jarvis-release-policy.json <<IAMPOL
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": ["s3:PutObject", "s3:PutObjectAcl", "s3:GetObject"],
      "Resource": "arn:aws:s3:::${BUCKET}/*"
    },
    {
      "Effect": "Allow",
      "Action": ["s3:ListBucket"],
      "Resource": "arn:aws:s3:::${BUCKET}"
    }
  ]
}
IAMPOL
aws iam put-user-policy \
  --user-name "${IAM_USER}" \
  --policy-name "jarvis-release-${BUCKET}" \
  --policy-document file:///tmp/jarvis-release-policy.json \
  --profile "${PROFILE}"

echo "==> Done. Bucket policy, CORS, versioning, lifecycle, and IAM user are configured."
echo "Public download URL pattern: https://${BUCKET}.s3.${REGION}.amazonaws.com/<file>"
