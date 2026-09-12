# ADR-003: Deposit state machine, finality, reorganization, and idempotency

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

A redundant Base indexer observes only allowlisted vault events. The canonical event identity is `(chain_id, transaction_hash, log_index)` with a unique database constraint. A second uniqueness constraint covers the vault's deposit ID.

Deposits transition through `observed -> confirming -> finalized -> credited`. Terminal or exceptional states are `rejected`, `orphaned`, `reversed`, and `quarantined`. Every transition is appended to deposit history; current state is a projection.

The indexer stores chain ID, block number and hash, transaction index and hash, log index, vault address, deposit ID, sender, destination account, asset address, and actual amount. It validates bytecode-pinned vault identity and event topics against a versioned deployment registry.

Credit occurs only after the event is returned under Base's `finalized` block tag and two independent RPC providers agree on the finalized block hash and decoded log. The ledger credit and transition to `credited` commit atomically under the event idempotency key.

Before credit, a reorg moves the event to `orphaned` without a ledger entry. A credited event that later contradicts canonical finalized history triggers an automatic asset halt and a compensating reversal into customer `pending` or `quarantined`, never silent deletion. Operators cannot manually mark a deposit finalized.

## Consequences

- Pending deposits are visible but unavailable for trading or withdrawal.
- Provider disagreement, stale heads, or indexer lag stops new credits.
- Backfills and restarts are safe because both event observation and ledger credit are idempotent.
