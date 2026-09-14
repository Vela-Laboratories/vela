# ADR-003: Deposit state machine, finality, reorganization, and idempotency

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

A redundant Base indexer observes only allowlisted vault events. The supported vault ABI emits `Deposited(bytes32 indexed depositId, address indexed user, address indexed asset, uint256 amount)`. The amount is the vault's measured balance delta for ERC20 deposits, not the caller's requested transfer amount. The canonical event identity is `(chain_id, transaction_hash, log_index)` with a unique database constraint. A second uniqueness constraint covers the deployment-scoped vault deposit ID.

Deposits transition through `observed -> confirming -> finalized -> credited`. Terminal or exceptional states are `rejected`, `orphaned`, `reversed`, and `quarantined`. Every transition is appended to deposit history; current state is a projection.

The indexer stores chain ID, block number and hash, transaction index and hash, log index, vault address, deposit ID, sender, destination account, asset address, and actual amount. It validates bytecode-pinned vault identity and event topics against a versioned deployment registry.

For launch-scope ETH and USDC, the confirmation policy is `rpc_finalized_two_provider`: credit occurs only after the event falls at or below the highest common Base `finalized` height and two independent RPC providers agree on that block hash and every normalized log in the scanned range. A numeric latest-head confirmation count is evidence only and never substitutes for finality. The ledger credit and transition to `credited` commit atomically under the event idempotency key.

Before credit, a reorg moves the event to `orphaned` without a ledger entry. A credited event that later contradicts canonical finalized history triggers an automatic asset halt and a compensating reversal into customer `pending` or `quarantined`, never silent deletion. Operators cannot manually mark a deposit finalized.

## Consequences

- Pending deposits are visible but unavailable for trading or withdrawal.
- Provider disagreement, stale heads, or indexer lag stops new credits.
- Backfills and restarts are safe because both event observation and ledger credit are idempotent.
