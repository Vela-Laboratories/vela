#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
snapshot=$(mktemp)
trap 'rm -f "$snapshot"' EXIT

cd "$repo_root"
cargo run --quiet -p api --bin dump-openapi > "$snapshot"
if ! diff -u docs/api-reference/openapi.json "$snapshot"; then
  echo "OpenAPI snapshot is stale. Regenerate docs/api-reference/openapi.json with dump-openapi." >&2
  exit 1
fi
