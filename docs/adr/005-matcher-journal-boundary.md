# ADR-005: Matching-engine integration and atomic journal boundary

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

The matching engine is a deterministic execution service, not a financial database. For each accepted command it produces a proposed effect set containing order changes, fills, balance transfers, fees, rebates, sequence numbers, and the pre/post state hashes.

A ledger coordinator validates and commits the complete effect set in one serializable PostgreSQL transaction. The transaction writes balanced journal legs, order and fill records, projection updates, and an outbox event. Only after commit does the coordinator acknowledge the command and advance the externally visible committed engine sequence.

Commands have globally unique idempotency keys. Effect sets are deterministic for `(engine_epoch, sequence, command)`. A retried commit returns the existing result. If ledger commit fails, the engine discards the uncommitted proposal and rebuilds from the last committed checkpoint plus journal/outbox events.

Order reservation occurs in the ledger before an order can rest or execute. Cancels and fills release or transfer locked amounts in the same transaction that records their final state. No distributed transaction is attempted between an in-memory engine and the ledger; the ledger commit is the sole commit point.

## Consequences

- Matching latency includes durable ledger commit or a deliberately bounded, journaled group-commit protocol introduced by a later ADR.
- WebSocket and HTTP success events are emitted from committed outbox records, not speculative engine state.
- Recovery can reconstruct balances, orders, fills, fees, and sequence exactly.
