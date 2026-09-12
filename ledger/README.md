# Vela authoritative ledger

This crate is the PostgreSQL financial source of truth described by ADR-001. It owns append-only journal transactions, balanced journal legs, materialized account balances, idempotency, and the transactional outbox. The matching engine is not allowed to acknowledge financially effective work until this ledger commits.

## Local verification

The integration tests create isolated databases through SQLx and therefore require a PostgreSQL administrator URL:

```sh
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p ledger
```

CI starts PostgreSQL 16 and runs the ledger tests as part of the workspace test suite. No production database is provisioned by this crate.

## Migration rules

- Migrations in `migrations/` are append-only after they have reached a shared environment.
- Never edit or renumber an applied migration. Add a new forward migration.
- Destructive changes require an expand, migrate, verify, contract sequence.
- Journal and audit-history rows are corrected with new compensating records, never an update or delete.
- A production migration must be rehearsed on a restored backup and include an explicit rollback or forward-fix procedure.
