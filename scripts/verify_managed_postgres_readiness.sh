#!/usr/bin/env bash
# Validate a nonsecret managed-PostgreSQL readiness attestation. This script is
# deliberately provider-neutral: it verifies that an operator recorded every
# required external control and evidence reference, but never connects to a
# database, reads credentials, or treats a local pass as hosting evidence.
set -euo pipefail

usage() {
  printf 'Usage: %s --environment <staging|production> <attestation.env>\n' "$0" >&2
  exit 64
}

fail() {
  printf 'managed PostgreSQL readiness preflight failed: %s\n' "$1" >&2
  exit 1
}

environment=''
attestation=''

while [ "$#" -gt 0 ]; do
  case "$1" in
    --environment)
      [ "$#" -ge 2 ] || usage
      environment="$2"
      shift 2
      ;;
    --help|-h)
      usage
      ;;
    -*)
      usage
      ;;
    *)
      [ -z "$attestation" ] || usage
      attestation="$1"
      shift
      ;;
  esac
done

case "$environment" in staging|production) ;; *) usage ;; esac
[ -n "$attestation" ] && [ -f "$attestation" ] || usage

required_keys='MANIFEST_VERSION ENVIRONMENT CUSTODY_MILESTONE DATABASE_ENGINE DATABASE_MAJOR_VERSION PRIMARY_FAILURE_DOMAIN STANDBY_FAILURE_DOMAIN OFFSITE_FAILURE_DOMAIN PRIVATE_NETWORKING TLS_ENFORCED WRITER_ROLE_SEPARATION READER_ROLE_SEPARATION MIGRATOR_ROLE_SEPARATION WRITER_CREDENTIAL_REFERENCE READER_CREDENTIAL_REFERENCE MIGRATOR_CREDENTIAL_REFERENCE MIGRATOR_APPROVAL SYNCHRONOUS_REPLICATION AUTOMATED_BACKUPS BACKUP_ENCRYPTION PITR PITR_RETENTION_DAYS OFFSITE_BACKUP_EXPORT OFFSITE_BACKUP_ENCRYPTION RESTORE_REHEARSAL PITR_REHEARSAL FAILOVER_REHEARSAL IN_FLIGHT_FINANCIAL_RECOVERY_REHEARSAL RESTORE_EVIDENCE_REFERENCE PAGING_DESTINATION PAGING_DELIVERY_REHEARSAL PAGING_EVIDENCE_REFERENCE RPO_TARGET LEDGER_RTO_MINUTES TRADING_RTO_MINUTES RUNBOOK_APPROVAL OPS_OWNER_REFERENCE'
seen_keys=''

while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in
    ''|'#'*) continue ;;
  esac

  case "$line" in
    *=*) key=${line%%=*}; value=${line#*=} ;;
    *) fail 'malformed attestation line' ;;
  esac

  case " $required_keys " in
    *" $key "*) ;;
    *) fail "unknown or disallowed field $key" ;;
  esac
  case " $seen_keys " in
    *" $key "*) fail "duplicate field $key" ;;
  esac
  seen_keys="$seen_keys $key"

  case "$value" in
    ''|REPLACE_ME|CHANGE_ME|TODO|*'://'*) fail "field $key is empty, a placeholder, or a credential/URL" ;;
  esac
  case "$value" in
    *[!A-Za-z0-9._:@/+,-]*) fail "field $key contains unsupported characters" ;;
  esac

  # Key and value have been restricted to a safe token alphabet. Do not source
  # an attestation: it is input data, not executable shell.
  eval "$key=\$value"
done < "$attestation"

for key in $required_keys; do
  eval "value=\${$key-}"
  [ -n "$value" ] || fail "missing required field $key"
done

[ "$MANIFEST_VERSION" = '1' ] || fail 'MANIFEST_VERSION must be 1'
[ "$ENVIRONMENT" = "$environment" ] || fail 'ENVIRONMENT does not match --environment'
[ "$CUSTODY_MILESTONE" = 'M5_V3_OMNIBUS' ] || fail 'CUSTODY_MILESTONE must be M5_V3_OMNIBUS'
[ "$DATABASE_ENGINE" = 'postgresql' ] || fail 'DATABASE_ENGINE must be postgresql'
[ "$DATABASE_MAJOR_VERSION" = '16' ] || fail 'DATABASE_MAJOR_VERSION must be 16'

for key in PRIVATE_NETWORKING TLS_ENFORCED WRITER_ROLE_SEPARATION READER_ROLE_SEPARATION MIGRATOR_ROLE_SEPARATION SYNCHRONOUS_REPLICATION AUTOMATED_BACKUPS BACKUP_ENCRYPTION PITR OFFSITE_BACKUP_EXPORT OFFSITE_BACKUP_ENCRYPTION; do
  eval "value=\${$key}"
  [ "$value" = 'enabled' ] || [ "$value" = 'verified' ] || fail "$key is not enabled or verified"
done

[ "$MIGRATOR_APPROVAL" = 'required' ] || fail 'MIGRATOR_APPROVAL must be required'
[ "$WRITER_CREDENTIAL_REFERENCE" != "$READER_CREDENTIAL_REFERENCE" ] || fail 'writer and reader credential references must differ'
[ "$WRITER_CREDENTIAL_REFERENCE" != "$MIGRATOR_CREDENTIAL_REFERENCE" ] || fail 'writer and migrator credential references must differ'
[ "$READER_CREDENTIAL_REFERENCE" != "$MIGRATOR_CREDENTIAL_REFERENCE" ] || fail 'reader and migrator credential references must differ'

case "$PITR_RETENTION_DAYS" in *[!0-9]*|'') fail 'PITR_RETENTION_DAYS must be an integer' ;; esac
[ "$PITR_RETENTION_DAYS" -ge 35 ] || fail 'PITR_RETENTION_DAYS must be at least 35'

[ "$PRIMARY_FAILURE_DOMAIN" != "$STANDBY_FAILURE_DOMAIN" ] || fail 'primary and standby failure domains must differ'
[ "$PRIMARY_FAILURE_DOMAIN" != "$OFFSITE_FAILURE_DOMAIN" ] || fail 'primary and offsite failure domains must differ'
[ "$STANDBY_FAILURE_DOMAIN" != "$OFFSITE_FAILURE_DOMAIN" ] || fail 'standby and offsite failure domains must differ'

for key in RESTORE_REHEARSAL PITR_REHEARSAL FAILOVER_REHEARSAL IN_FLIGHT_FINANCIAL_RECOVERY_REHEARSAL PAGING_DELIVERY_REHEARSAL; do
  eval "value=\${$key}"
  [ "$value" = 'passed' ] || fail "$key must be passed"
done
[ "$PAGING_DESTINATION" = 'verified' ] || fail 'PAGING_DESTINATION must be verified'
[ "$RPO_TARGET" = 'zero_acknowledged_operations' ] || fail 'RPO_TARGET must be zero_acknowledged_operations'
[ "$LEDGER_RTO_MINUTES" = '30' ] || fail 'LEDGER_RTO_MINUTES must be 30'
[ "$TRADING_RTO_MINUTES" = '60' ] || fail 'TRADING_RTO_MINUTES must be 60'
[ "$RUNBOOK_APPROVAL" = 'two_person' ] || fail 'RUNBOOK_APPROVAL must be two_person'

printf 'managed PostgreSQL readiness preflight passed for %s (nonsecret attestation only)\n' "$environment"
