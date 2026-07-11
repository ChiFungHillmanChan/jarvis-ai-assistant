# JARVIS Infra (AWS CDK)

Cloud control plane for JARVIS. Deploys to account `759626151650` in `ap-southeast-1`.

## Stacks

- `Jarvis-<env>-Auth` -- Cognito user pool + KMS CMK
- `Jarvis-<env>-Data` -- S3 sync bucket + DynamoDB user meta + plugin registry
- `Jarvis-<env>-Observability` -- CloudWatch dashboard + budget alarms + SNS

`<env>` is one of `sandbox` / `dev` / `prod`.

## First-time setup

Pre-req: `jarvis-cdk-deployer` IAM user with admin policy（root account 開）。Local profile `jarvis-deploy` configured.

```bash
cd infra
npm install
npm run build

# Bootstrap once per account/region
npm run bootstrap

# Deploy sandbox first to test
npm run deploy:sandbox
```

## 預估 monthly cost (sandbox / dev)

| Resource | Cost |
|----------|------|
| KMS CMK | $1 |
| S3 (5GB free tier) | $0 |
| DynamoDB pay-per-request (low traffic) | <$1 |
| Cognito (free tier 50k MAU) | $0 |
| SNS + budgets | $0 |
| CloudWatch dashboard | $3 |
| **Total** | **~$5/mo per env** |

Activate credits cover this 100x.

## Destroy sandbox

```bash
npm run destroy:sandbox
```

`prod` stack 唔可以 destroy（KMS + S3 + DynamoDB removalPolicy = RETAIN）。

## Outputs

After deploy, outputs 寫入 `cdk.out/<stack>.out.json`. Tauri build script reads:
- `UserPoolId`, `UserPoolClientId` -> Cognito SDK config
- `DataKeyArn` -> envelope encryption client
- `SyncBucketName` -> sync target

See `scripts/sync-cdk-outputs.sh`.

## Releases bucket (public read)

The `jarvis-releases` bucket (`ap-east-1`, separate from the CDK stacks above)
holds the signed DMG + updater tarball. `scripts/release.sh` uploads there. The
in-app updater and the website `/download` page must be able to GET these
objects, so the bucket needs public-read. New S3 buckets have Block Public
Access ON, which overrides bucket policies, so both steps are required:

```bash
BUCKET=jarvis-releases
PROFILE=jarvis-release-bot   # or your releases profile

# 1. Let a bucket policy grant public access (turn off the blockers).
aws s3api put-public-access-block --bucket "$BUCKET" \
  --public-access-block-configuration \
  BlockPublicAcls=false,IgnorePublicAcls=false,BlockPublicPolicy=false,RestrictPublicBuckets=false \
  --profile "$PROFILE"

# 2. Apply the public-read policy (public GET on objects only; no listing/writes).
aws s3api put-bucket-policy --bucket "$BUCKET" \
  --policy file://infra/releases-bucket-policy.json \
  --profile "$PROFILE"

# 3. Verify a public download works (no credentials).
curl -sI "https://$BUCKET.s3.ap-east-1.amazonaws.com/JARVIS_latest_aarch64.dmg" | head -1
```

The update *manifest* (`latest.json`) is NOT here — it is served publicly from
the website at `https://jarvis-automation.com/latest.json`. Only the binaries
live in this bucket.
