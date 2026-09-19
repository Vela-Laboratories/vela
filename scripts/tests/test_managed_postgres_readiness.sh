#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
preflight="$repository_root/scripts/verify_managed_postgres_readiness.sh"
fixtures="$repository_root/scripts/tests/fixtures/managed-postgres-readiness"

"$preflight" --environment production "$fixtures/valid-production.env"

if "$preflight" --environment production "$fixtures/missing-pitr.env" >/dev/null 2>&1; then
  printf 'expected a missing PITR control to fail closed\n' >&2
  exit 1
fi

if "$preflight" --environment staging "$fixtures/valid-production.env" >/dev/null 2>&1; then
  printf 'expected an environment mismatch to fail closed\n' >&2
  exit 1
fi

if output="$(sed 's|WRITER_CREDENTIAL_REFERENCE=secret-ref-writer-production|WRITER_CREDENTIAL_REFERENCE=postgres://not-a-secret-here|' "$fixtures/valid-production.env" | "$preflight" --environment production /dev/stdin 2>&1)"; then
  printf 'expected a credential-like value to fail closed\n' >&2
  exit 1
fi
case "$output" in
  *postgres://*)
    printf 'preflight must not echo a rejected credential-like value\n' >&2
    exit 1
    ;;
esac

printf 'managed PostgreSQL readiness preflight tests passed\n'
