#!/usr/bin/env bash
# Publishes a signed registry to one environment's registry bucket. Runs only
# in the Registry workflow's publish jobs, after the sign job, with the
# GitHub OIDC role for that environment (s3:GetObject/PutObject on
# registry/* and s3:ListBucket under registry/).
#
#   REGISTRY_BUCKET=<bucket> registry/publish.sh <dir with registry.json and registry.json.sig>
#
# Writes registry/v<version>.json and .sig once (never overwritten), then
# registry/latest.json.sig and registry/latest.json. latest.json also carries
# the signature as object metadata, which is what GET /v1/registry serves, so
# a reader never sees one version's body with another's signature.
# Re-running for a version that is already published with the same bytes
# changes nothing; any other content for an existing version is refused.
# Before any S3 call, the signature must verify with the committed public key
# (registry/signing-public-key.pem); a failed S3 list call stops the publish.
set -euo pipefail

dir="${1:?usage: publish.sh <signed dir>}"
bucket="${REGISTRY_BUCKET:?REGISTRY_BUCKET is not set (GitHub environment variable)}"
body="$dir/registry.json"
sig_file="$dir/registry.json.sig"
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

version="$(node -p 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).version' "$body")"
signature="$(tr -d '\n' < "$sig_file")"
[[ "$version" =~ ^[1-9][0-9]*$ ]] || { echo "registry: bad version '$version'" >&2; exit 1; }
[[ "$signature" =~ ^[A-Za-z0-9+/]{86}==$ ]] || { echo 'registry: bad signature file' >&2; exit 1; }
metadata="{\"signature\":\"$signature\",\"version\":\"$version\"}"

# The signature must verify over these exact bytes with the key the app
# trusts before anything is read from or written to the bucket.
node --experimental-strip-types "$here/verify.ts" "$body" "$sig_file"

# Whether this exact key is in the bucket. A failed list call stops the
# publish: read as "not there", it would skip the version check below.
exists() {
  local n
  n="$(aws s3api list-objects-v2 --bucket "$bucket" --prefix "$1" --query "length(Contents[?Key=='$1'] || \`[]\`)" --output text)" \
    || { echo "registry: cannot list s3://$bucket/$1; nothing is uploaded" >&2; exit 1; }
  [[ "$n" == 1 ]]
}

# Writes $1 from $2 only if the key does not exist yet; an existing object
# must hold the same bytes.
put_once() {
  local key="$1" file="$2" type="$3"
  shift 3
  if exists "$key"; then
    aws s3api get-object --bucket "$bucket" --key "$key" "$work/existing" > /dev/null
    if cmp -s "$work/existing" "$file"; then echo "registry: $key is already published"; return 0; fi
    echo "registry: $key already exists with other content; a published version is never replaced" >&2
    exit 1
  fi
  aws s3api put-object --bucket "$bucket" --key "$key" --body "$file" --content-type "$type" --if-none-match '*' "$@" > /dev/null
}

if exists registry/latest.json; then
  aws s3api get-object --bucket "$bucket" --key registry/latest.json "$work/published.json" > /dev/null
  if cmp -s "$work/published.json" "$body"; then
    echo "registry: version $version is already the published version"
    exit 0
  fi
  node --experimental-strip-types "$here/validate.ts" "$body" --published "$work/published.json"
else
  echo 'registry: nothing published yet; this is the first version'
  node --experimental-strip-types "$here/validate.ts" "$body"
fi

put_once "registry/v$version.json" "$body" application/json --metadata "$metadata"
put_once "registry/v$version.json.sig" "$sig_file" text/plain
aws s3api put-object --bucket "$bucket" --key registry/latest.json.sig --body "$sig_file" --content-type text/plain > /dev/null
aws s3api put-object --bucket "$bucket" --key registry/latest.json --body "$body" --content-type application/json --metadata "$metadata" > /dev/null
echo "registry: published version $version to s3://$bucket/registry/latest.json"
