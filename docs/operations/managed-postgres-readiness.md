# Managed PostgreSQL resilience gate

This is the EX-014 operational gate for a future managed PostgreSQL 16
deployment. It is provider-neutral and does not select, provision, or contact a
provider. The public Vela deployment remains a testnet demo. A passing local
preflight is not evidence that a hosted database, audit, deployment, or paging
receiver exists.

The `CUSTODY_MILESTONE` in this gate is deliberately `M5_V3_OMNIBUS`: M5 is the
V3 omnibus-custody boundary. M6 resilience work protects that boundary. Do not
reuse this attestation to imply that the legacy M6 roadmap, a V2 design, or a
production launch has completed.

## What must be externally provisioned

Create completely isolated staging and production PostgreSQL 16 services. For
each environment, independently verify all of the following before recording a
passed attestation:

| Control | Required evidence |
| --- | --- |
| Isolation | Private or equivalently restricted network path, TLS enforcement, distinct environment accounts/namespaces, and no public database endpoint. |
| Least privilege | Separate writer, reader, and migrator login identities; the API receives only the writer secret; the migrator secret is available only to the manually approved migration path. |
| Failure domains | Synchronous PostgreSQL replication between distinct primary and standby failure domains, plus an offsite backup target in a third distinct failure domain. |
| Backup and PITR | Automated encrypted backups, encrypted offsite export, PostgreSQL 16 point-in-time recovery, and at least 35 days of retained recovery history. |
| Restore proof | A recorded restore from a production-like backup and a restore to a named point before a deliberately later write. The record includes source/target environment IDs, PostgreSQL version, migration version, target point, restored commit/transaction boundary, observed RPO/RTO, operator, reviewer, and immutable evidence location. |
| Failure proof | A failover exercise and a controlled recovery while deposits, orders, trades, and withdrawals are in flight. Verify the journal, balances, withdrawal states, reconciliation result, asset halts, and readiness before admitting traffic. |
| Paging | A separately operated paging destination with tested delivery for failed writes, replication lag/failure, backup/PITR failure, database saturation, and readiness loss. Record the delivery-exercise evidence and escalation owner. |
| Approval | Two people approve the runbook and each production migration/deployment. No service automatically clears financial halts after recovery. |

Do not put a database URL, password, key, pager token, provider console URL, or
customer data in source control or in the attestation. Store credential values
only in the approved secret system. The attestation records opaque secret and
evidence references, not the referenced material.

## Fail-closed preflight

Copy [the template](templates/managed-postgres-readiness.env) into an
access-controlled operations system for each environment. The checked-in
template intentionally fails. A completed file must remain outside this
repository and contain only opaque references such as an incident or change
record ID.

Run the preflight after the external evidence has been reviewed:

```sh
./scripts/verify_managed_postgres_readiness.sh \
  --environment staging /secure/ops/vela-postgres-staging-attestation.env

./scripts/verify_managed_postgres_readiness.sh \
  --environment production /secure/ops/vela-postgres-production-attestation.env
```

The preflight rejects absent fields, placeholders, URLs and credential-like
values, an environment mismatch, PostgreSQL other than 16, missing private
networking/TLS/role separation, shared failure-domain labels, disabled backup
or PITR controls, less than 35 days of PITR retention, absent proof fields,
unpassed restore/failover/in-flight/paging exercises, mismatched RPO/RTO, or a
missing two-person runbook approval. It prints control names only, never values.

It deliberately cannot inspect a provider, authenticate to PostgreSQL, measure
replication lag, or establish that an evidence reference is truthful. Treat a
pass as a syntactic completeness check on a reviewed, nonsecret attestation,
not as a substitute for the underlying exercise.

## Admission sequence

1. Complete staging first, including controlled migrations, the restore and
   named-point PITR rehearsal, failover, in-flight financial recovery, and a
   real paging delivery test.
2. Review the evidence with the database, custody, and incident owners. Update
   the staging attestation and run the preflight.
3. Repeat against the isolated production environment. Rehearse the current
   release and migration version, not merely an empty schema.
4. Before any traffic change, verify ledger schema readiness, recovery-bundle
   verification, reconciliation, active asset halts, and every required
   dependency endpoint. Keep traffic blocked on any failure.
5. Preserve the evidence, approvals, and selected recovery artifacts for the
   post-exercise review. Repeat at least quarterly and after any topology,
   provider, PostgreSQL-major-version, or recovery-procedure change.

The local `scripts/rehearse_postgres_pitr.sh` CI job and this preflight are
repository controls only. Neither replaces managed-provider restore evidence.
See [the ledger database runbook](ledger-database.md) and
[recovery/failover runbook](engine-recovery-replication.md) for the runtime
procedure.
