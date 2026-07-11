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

The `jarvis-releases` bucket (`ap-east-1`, account 759626151650, separate from
the CDK stacks above) holds the signed DMG + updater tarball. `scripts/release.sh`
uploads there via the `jarvis-release-bot` profile.

**This is already configured** — the bucket has a scoped public-read policy (the
one mirrored in `releases-bucket-policy.json`: public `s3:GetObject` on the DMG /
tarball / `.sig` / `latest.json` only, no listing or writes) and Block Public
Access allows it (`BlockPublicPolicy=false`). Nothing to do for a normal release;
`release.sh` just uploads and the objects are publicly downloadable.

Re-apply the policy only if it is ever lost (use the `jarvis-deploy` profile,
which has bucket-policy permissions; `jarvis-release-bot` is upload-only):

```bash
aws s3api put-bucket-policy --bucket jarvis-releases \
  --policy file://infra/releases-bucket-policy.json \
  --profile jarvis-deploy
```

The update *manifest* (`latest.json`) that the app reads is NOT here — it is
served publicly from the website at `https://jarvis-automation.com/latest.json`.
This bucket only serves the binaries (and an optional mirror of the manifest).
