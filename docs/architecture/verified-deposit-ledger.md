# Verified deposit ledger boundary

## Scope

The durable ledger commands are now connected to a dedicated Base indexer. The process validates the immutable deployment registry, runtime bytecode hash, exact event ABI, provider health, and two independent RPC views before it calls these commands. It is implemented and locally verified, but it is not a production deployment: the updated vault still needs audit and Base Sepolia deployment, and protected database/RPC configuration still needs provisioning.

## Observation identity

`Ledger::observe_deposit` records the complete custody event and initial history entry in one serializable transaction. The canonical identity is `(chain_id, lower(transaction_hash), log_index)`; the vault deposit ID and internal deposit ID are independently unique. Exact retries return the existing deposit, while any reuse with different customer, account, asset, amount, block, vault, sender, or event data fails closed.

The destination must be the named customer's nonnegative `customer_available` account for the configured asset. The observed chain and asset contract must match the ledger asset registry, and the declared amount must exactly equal the integer base-unit amount emitted by the vault.

## State and finality

The typed state model follows ADR-003: `observed`, `confirming`, `finalized`, `credited`, `rejected`, `orphaned`, `reversed`, and `quarantined`. PostgreSQL rejects invalid transitions and changes to immutable deposit identity or amount. A deferred constraint requires every initial state and transition to have an immutable history row written in the same transaction.

Only the dedicated finality command can move a confirming deposit to finalized. It requires two distinctly named RPC providers to report the same finalized block number and hash, deposit block hash, transaction hash, and log index. The evidence must also match the stored canonical event, and the finalized height cannot precede the deposit block.

A pre-credit reorganization moves the deposit to `orphaned` and cannot produce a balance. Previously processed finalized ranges are also re-audited continuously against both providers. If a credited event disappears or its immutable facts change, the ledger atomically activates an asset-wide halt before isolating the customer credit.

If the complete credited amount remains available, a compensating `deposit_reversal` journal entry moves it from `customer_available` to `customer_pending` and the deposit becomes `reversed`. If some of the amount has already moved, every remaining available unit is moved to pending and the deposit becomes `quarantined` with the exact shortfall in immutable evidence. No partial result is described as a successful reversal.

## Atomic credit

`Ledger::credit_finalized_deposit` accepts only `finalized` deposits. It verifies a same-asset, negative-enabled `custody_vault` account and atomically:

1. posts the custody debit and customer-available credit;
2. updates the materialized balance projections;
3. writes the transactional outbox event;
4. changes the deposit to `credited` and binds its journal transaction;
5. appends the immutable state-history record.

The idempotency key is derived from canonical chain-event identity and its request hash binds the deposit, destination account, custody account, amount, and key. Retrying returns the original journal transaction without applying a second credit. The controlled bootstrap provisions per-asset custody accounts for the configured vault.

## Indexer boundary

Migration 0003 adds an append-only deployment registry, a single-worker lease, a monotonic restart checkpoint, and immutable halt evidence. Migration 0004 adds a separately guarded cyclic audit checkpoint, credited-deposit reversal references, and durable per-asset halts. The indexer scans only through the providers' highest common finalized height. It rejects stale latest heads, excessive provider lag, wrong chain IDs, finalized hash disagreement or regression, runtime bytecode mismatch, malformed or duplicate logs, out-of-range results, unregistered assets, and ledger rejection. Forward checkpoints advance only after every event in the range reaches an idempotent terminal credit result. Audit checkpoints advance only after the already-processed range still agrees with both providers.

An active asset halt blocks new deposit credits, order reservations, trades, fees, rebates, withdrawal reservations, and withdrawal completions at the PostgreSQL journal-leg boundary. Releases and deposit reversals remain permitted so the system can reduce exposure. Halt evidence is append-only; resolution requires evidence and two distinct actor references, and the restricted runtime writer cannot resolve it.

The legacy Sepolia deployment emits the older three-field event and is intentionally incompatible. Only a newly deployed contract with `Deposited(bytes32,address,address,uint256)` can be registered.

## Remaining launch work

The ledger-backed customer deposit-status API and frontend transaction discovery are implemented. Base Sepolia contract deployment, managed indexer hosting, protected provider/database configuration, custody reconciliation, and operational alerting remain Milestone 3 blockers. The frontend therefore keeps verified submission disabled until an explicitly configured Base vault and asset address match the indexer registry.
