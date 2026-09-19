#!/usr/bin/env bash
set -euo pipefail

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
index="$repo_root/docs/operations/m8-launch-evidence.md"
template="$repo_root/docs/operations/templates/m8-evidence-record.md"

test -s "$index"
test -s "$template"
grep -q 'V3 `VelaSettlement` omnibus vault' "$index"
grep -q 'External gates intentionally unchecked' "$index"
grep -q 'Capped adversarial testnet' "$index"
grep -q 'Incident rehearsal' "$index"
grep -q 'Reviewed commit' "$template"
grep -q 'Independent review completed' "$template"

# The scoped documentation must not reintroduce the retired V2 contract label.
for doc in \
  "$repo_root/docs/architecture/verified-withdrawal-ledger.md" \
  "$repo_root/docs/operations/withdrawal-observer.md"; do
  if grep -Eq 'V2 (contract|custody|deployment|observer|withdrawal|testnet)' "$doc"; then
    echo "retired V2 contract wording found in $doc" >&2
    exit 1
  fi
done

echo "M8 evidence package checks passed"
