# ADR-001: Authoritative ledger and consistency model

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

Vela will use PostgreSQL as the authoritative financial system of record. An append-only, double-entry journal records every deposit, trade, fee, rebate, withdrawal, and administrative adjustment. Customer balances, matcher balances, statements, and proof inputs are projections derived from that journal.

Each journal transaction has a globally unique ID, an operation type, an idempotency key, timestamps, external references, and two or more immutable legs whose signed amounts sum to zero per asset. Amounts use integer base units and never floating point.

Financial commands execute in `SERIALIZABLE` database transactions. Rows for affected account balances are locked in a stable order. The transaction verifies sufficient available funds, writes journal headers and legs, updates balance projections, and records an outbox event before commit. A response that implies financial success is sent only after commit.

Journal rows are never updated or deleted. Corrections use compensating transactions. Database roles deny application code `UPDATE` and `DELETE` on journal tables after insertion.

## Consequences

- The current matching-engine balance map and WAL are not authoritative and cannot independently acknowledge money movement.
- Read projections may be eventually consistent, but authorization and available-balance checks use the serializable ledger transaction.
- Every write API needs an idempotency key and deterministic replay behavior.
- PostgreSQL availability becomes a hard dependency; financial operations fail closed when it is unavailable.
