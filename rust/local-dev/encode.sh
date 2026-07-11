#!/usr/bin/env bash
# encode.sh — base64-encode a JSON config/secrets file and emit a Kubernetes
# ConfigMap or Secret manifest containing it under a given key. Recreates the
# tool referenced (but not included) in the original Python deployment docs;
# every service's Ctx::load() expects exactly this shape — base64(JSON) — so
# it works unchanged for the Rust services.
#
# Usage:
#   ./encode.sh -f <input.json> -y <output.yaml> -k <key> \
#     [-n <k8s-name>] [-N <namespace>] [-s]
#
#   -f  input JSON file (config or secrets)
#   -y  output manifest path
#   -k  data key the value is stored under (must match what the SpinApp's
#       configMapKeyRef/secretKeyRef references — see local-dev/README.md
#       for the exact key name per service)
#   -n  Kubernetes object name (default: derived from -k, replacing
#       underscores with hyphens, e.g. account_config -> account-config)
#   -N  namespace (default: octy-local)
#   -s  emit a Secret instead of a ConfigMap (default: ConfigMap)
#
# Example:
#   ./encode.sh -f config/account.config.json -y /tmp/account-configmap.yaml \
#     -k ACCOUNT_CONFIG -n account-config
#   ./encode.sh -f config/account.secrets.json -y /tmp/account-secrets.yaml \
#     -k ACCOUNT_SECRETS -n account-secrets -s

set -euo pipefail

namespace="octy-local"
is_secret=false
name=""

while getopts "f:y:k:n:N:s" opt; do
  case "$opt" in
    f) infile="$OPTARG" ;;
    y) outfile="$OPTARG" ;;
    k) key="$OPTARG" ;;
    n) name="$OPTARG" ;;
    N) namespace="$OPTARG" ;;
    s) is_secret=true ;;
    *) echo "usage: $0 -f <input.json> -y <output.yaml> -k <key> [-n <name>] [-N <namespace>] [-s]" >&2; exit 1 ;;
  esac
done

if [[ -z "${infile:-}" || -z "${outfile:-}" || -z "${key:-}" ]]; then
  echo "error: -f, -y, and -k are required" >&2
  exit 1
fi

if [[ -z "$name" ]]; then
  name=$(echo "$key" | tr '[:upper:]_' '[:lower:]-')
fi

encoded=$(base64 < "$infile" | tr -d '\n')

if [[ "$is_secret" == "true" ]]; then
  kind="Secret"
  field="data"
else
  kind="ConfigMap"
  field="data"
fi

cat > "$outfile" <<EOF
apiVersion: v1
kind: $kind
metadata:
  name: $name
  namespace: $namespace
$field:
  $key: $encoded
EOF

echo "wrote $outfile ($kind/$name, key $key, namespace $namespace)"
