#!/usr/bin/env bash
# Refuses to deploy while Bedrock model invocation logging is on in any catalog
# region: that setting would write user prompts and replies to CloudWatch or S3.
# Fails closed if the AWS call itself errors.
set -euo pipefail

cd "$(dirname "$0")/.."

regions=$(node -e "
const c = require('./config/jarvis-ai.json').catalog;
const regions = Object.values(c).map(m => m && m.region);
if (!regions.every(r => typeof r === 'string' && r !== '')) {
  console.error('A catalog model has no region; refusing to deploy');
  process.exit(1);
}
console.log([...new Set(regions)].join('\n'));
")
[ -n "$regions" ] || { echo "No catalog regions found; refusing to deploy" >&2; exit 1; }

for r in $regions; do
  if ! out=$(aws bedrock get-model-invocation-logging-configuration \
      --region "$r" --profile "${AWS_PROFILE:-jarvis}" \
      --query loggingConfig --output json); then
    echo "Could not read Bedrock invocation logging in $r; refusing to deploy" >&2
    exit 1
  fi
  if [ "$out" != "null" ]; then
    echo "Bedrock invocation logging is ON in $r; refusing to deploy" >&2
    exit 1
  fi
  echo "Bedrock invocation logging is off in $r"
done
