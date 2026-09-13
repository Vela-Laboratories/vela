# Vela authoritative ledger

This crate is the PostgreSQL financial source of truth described by ADR-001. It owns append-only journal transactions, balanced journal legs, materialized account balances, idempotency, and the transactional outbox. The matching engine is not allowed to acknowledge financially effective work until this ledger commits.

## Local verification

The ledger and matcher integration tests create isolated databases through SQLx and therefore require a PostgreSQL administrator URL:

```sh
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p ledger -p api --test ledger_financial_commit
```

CI starts PostgreSQL 16 and runs the ledger tests as part of the workspace test suite. No production database is provisioned by this crate.

## Runtime configuration

The API uses `DATABASE_URL` only for the restricted application writer. It
creates a bounded pool, applies a statement timeout to every connection, and
retries serializable conflicts and deadlocks a bounded number of times. In
production, startup fails when `DATABASE_URL` is absent or the schema is not at
the exact version expected by the binary.

Migrations use a separate privileged secret and an explicit command:

```sh
DATABASE_MIGRATOR_URL=postgres://... cargo run --locked -p ledger --bin ledger-migrate
```

The API never reads `DATABASE_MIGRATOR_URL` and never runs migrations at
startup. After the initial migration, apply `sql/roles.sql` as the database
owner. Create separate secret-bearing login roles outside the script and grant
each login exactly one of `vela_ledger_migrator`, `vela_ledger_writer`, or
`vela_ledger_reader`.

`GET /health` is process liveness. `GET /ready` checks database connectivity and
the exact ledger schema version whenever the ledger is configured. Production
returns not-ready when the ledger is absent or unhealthy; development and the
public testnet demo may run without a ledger. When configured, the matcher
keeps each batch private until its complete balanced financial effect commits;
failure discards the proposal and returns an internal error.

See [the matcher-ledger commit boundary](../docs/architecture/matcher-ledger-commit.md)
for account requirements, decimal conversion, acknowledgement ordering, and
current limitations.

See [the ledger database runbook](../docs/operations/ledger-database.md) for
setup, migration, access, and recovery procedures.

## Migration rules

- Migrations in `migrations/` are append-only after they have reached a shared environment.
- Never edit or renumber an applied migration. Add a new forward migration.
- Destructive changes require an expand, migrate, verify, contract sequence.
- Journal and audit-history rows are corrected with new compensating records, never an update or delete.
- A production migration must be rehearsed on a restored backup and include an explicit rollback or forward-fix procedure.
