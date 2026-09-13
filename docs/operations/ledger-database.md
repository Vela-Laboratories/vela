# Ledger database runbook

## Scope and current limit

This runbook covers the zero-cost database runtime layer: local PostgreSQL 16,
controlled migrations, restricted roles, health behavior, and a logical
backup/restore rehearsal. It does not provision a managed staging or production
database. The current Fly volume and matcher WAL are not acceptable substitutes
for an authoritative production ledger.

Do not accept real customer funds until a managed PostgreSQL deployment has
replication across failure domains, encrypted offsite backups, point-in-time
recovery, monitoring, and a successfully rehearsed provider-specific restore.

## Local PostgreSQL

Start an isolated development database without committing its password:

```sh
docker run --name vela-ledger-postgres --rm \
  -e POSTGRES_PASSWORD="$(openssl rand -hex 24)" \
  -p 127.0.0.1:5432:5432 \
  -d postgres:16-alpine
```

Use a locally held administrator URL for tests and initial setup. Never paste a
real database URL into source files, shell history, logs, issue comments, or CI
output.

## Controlled migration

The application does not migrate its own database. Run the dedicated migrator
with a privileged URL:

```sh
DATABASE_MIGRATOR_URL=postgres://... \
  cargo run --locked --release -p ledger --bin ledger-migrate
```

The command applies only checked-in SQLx migrations and then verifies that the
database is at the exact schema version expected by the binary. For shared
environments, dispatch `.github/workflows/ledger-migrate.yml`. Store
`DATABASE_MIGRATOR_URL` as an environment secret and require approval on the
production GitHub Environment.

Migration rules:

- Never edit or renumber a migration after it reaches a shared environment.
- Use expand, migrate, verify, contract for destructive changes.
- Rehearse every production migration against a restored backup.
- Write an explicit forward-fix or rollback procedure before approval.
- Deploy schema-compatible application code around the migration window.

## Asset and platform-account bootstrap

After migration, run the idempotent bootstrap command with the same controlled migrator credential:

```sh
DATABASE_MIGRATOR_URL=postgres://... \
VELA_CHAIN_ID=8453 \
VELA_USDC_ADDRESS=0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913 \
VELA_CUSTODY_VAULT_ADDRESS=0x... \
cargo run --locked --release -p ledger --bin ledger-bootstrap
```

Use the chain ID, native USDC contract, and allowlisted omnibus vault for the target environment. The command provisions ETH, USDC, `platform:vela` fee/rebate accounts, and negative-enabled custody-vault counter-accounts for both assets. It refuses to overwrite an existing asset definition or account policy with different configuration. Matcher commits may create missing customer available/locked accounts for these approved assets, but cannot introduce a new asset.

The manually approved GitHub migration workflow runs migration and bootstrap in sequence. Configure `VELA_CHAIN_ID`, `VELA_USDC_ADDRESS`, and `VELA_CUSTODY_VAULT_ADDRESS` as environment variables and keep `DATABASE_MIGRATOR_URL` as an environment secret.

## Database roles and secrets

After the first migration, apply the role policy as the database owner:

```sh
psql "$DATABASE_OWNER_URL" -v ON_ERROR_STOP=1 -f ledger/sql/roles.sql
```

`roles.sql` creates three non-login group roles:

- `vela_ledger_migrator` owns the schema and financial objects.
- `vela_ledger_writer` can perform only the reads and writes required by the
  current ledger implementation. It cannot alter migration history or delete
  journal records.
- `vela_ledger_reader` is read-only for reconciliation and investigation.

Create distinct provider login identities and grant each login one group role.
Use the writer URL as `DATABASE_URL` for the API and the migrator URL as
`DATABASE_MIGRATOR_URL` only for the migration workflow. Do not share passwords
between environments or roles. New migrations must explicitly extend grants for
new objects, so unexpected access fails closed.

## Runtime and health behavior

The API uses a bounded connection pool and per-connection statement timeout.
Defaults and overrides are documented in `docs/env-vars.md`. Financial postings
run in serializable transactions and retry serialization failures and deadlocks
with bounded exponential backoff.

- `/health` proves only that the process can answer requests.
- `/ready` verifies database connectivity and the exact migration version when
  a ledger is configured.
- Production startup rejects a missing `DATABASE_URL`.
- Production readiness rejects a missing, unreachable, or stale-schema ledger.
- Development and `testnet-demo` may omit the ledger; such runs are explicitly non-authoritative.
- When configured, startup replays the immutable journal and independently compares every result with its materialized account projection.
- Replay divergence, invalid customer ownership, overflow, or inexact asset conversion aborts ledger-backed startup.

Do not add the migrator credential to the API runtime. Database errors returned
by readiness are intentionally sanitized; use protected service logs for the
internal cause.

## Logical backup and restore rehearsal

The following rehearsal validates the repository-level recovery mechanics. It
does not prove managed-provider point-in-time recovery or cross-region failover.
Run it with temporary database names in an isolated PostgreSQL instance:

```sh
backup_file="$(mktemp -t vela-ledger.XXXXXX.dump)"
pg_dump "$DATABASE_OWNER_URL" --format=custom --no-owner --file="$backup_file"

createdb vela_ledger_restore
pg_restore --dbname=vela_ledger_restore --no-owner "$backup_file"
psql postgres:///vela_ledger_restore -v ON_ERROR_STOP=1 -c \
  "SELECT max(version) FROM _sqlx_migrations WHERE success"
psql postgres:///vela_ledger_restore -v ON_ERROR_STOP=1 -c \
  "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name LIKE 'ledger_%'"
```

Then run the ledger integration tests against the restored server, inspect row
counts and reconciliation outputs for the rehearsal dataset, apply `roles.sql`,
and verify writer, reader, and migrator permissions. Remove the temporary dump
and restored database only after recording the evidence.

On 2026-09-12 this procedure restored schema version 1 and all 15 ledger tables
into a separate local PostgreSQL 16 database. The role smoke test also proved
writer insert/read access, denial of journal deletion, reader read-only access,
and migrator DDL access. This is local evidence only; the managed database,
offsite backup, point-in-time recovery, and disaster-recovery rehearsal remain
open launch requirements.

## Managed-environment completion gate

Before marking managed database provisioning complete:

1. Provision isolated staging and production PostgreSQL 16 databases.
2. Configure private networking or an equivalent restricted connection path.
3. Install separate migrator, writer, and reader identities and verify grants.
4. Enable encryption, multi-zone replication, offsite backups, and point-in-time
   recovery with documented retention.
5. Add database saturation, replication lag, failed backup, and readiness alerts.
6. Restore a production-like backup into an isolated environment and record RPO
   and RTO evidence.
7. Prove application fail-closed behavior during database loss and stale schema.
8. Require reviewed migration and deployment approvals.
