# Verified deposit ledger boundary

## Scope

This chapter establishes the durable half of the Base deposit pipeline. It does not claim that a production chain indexer exists. A future indexer is the only intended caller of these ledger commands and must still validate the deployed vault bytecode, event signature, provider health, and deployment registry.

## Observation identity

`Ledger::observe_deposit` records the complete custody event and initial history entry in one serializable transaction. The canonical identity is `(chain_id, lower(transaction_hash), log_index)`; the vault deposit ID and internal deposit ID are independently unique. Exact retries return the existing deposit, while any reuse with different customer, account, asset, amount, block, vault, sender, or event data fails closed.

The destination must be the named customer's nonnegative `customer_available` account for the configured asset. The observed chain and asset contract must match the ledger asset registry, and the declared amount must exactly equal the integer base-unit amount emitted by the vault.

## State and finality

The typed state model follows ADR-003: `observed`, `confirming`, `finalized`, `credited`, `rejected`, `orphaned`, `reversed`, and `quarantined`. PostgreSQL rejects invalid transitions and changes to immutable deposit identity or amount. A deferred constraint requires every initial state and transition to have an immutable history row written in the same transaction.

Only the dedicated finality command can move a confirming deposit to finalized. It requires two distinctly named RPC providers to report the same finalized block number and hash, deposit block hash, transaction hash, and log index. The evidence must also match the stored canonical event, and the finalized height cannot precede the deposit block.

A pre-credit reorganization moves the deposit to `orphaned` and cannot produce a balance. Compensating reversal of an already credited deposit, automatic asset halts, and reconciliation are deliberately not implemented in this chapter.

## Atomic credit

`Ledger::credit_finalized_deposit` accepts only `finalized` deposits. It verifies a same-asset, negative-enabled `custody_vault` account and atomically:

1. posts the custody debit and customer-available credit;
2. updates the materialized balance projections;
3. writes the transactional outbox event;
4. changes the deposit to `credited` and binds its journal transaction;
5. appends the immutable state-history record.

The idempotency key is derived from canonical chain-event identity and its request hash binds the deposit, destination account, custody account, amount, and key. Retrying returns the original journal transaction without applying a second credit. The controlled bootstrap provisions per-asset custody accounts for the configured vault.

## Remaining launch work

The dedicated Base indexer, versioned deployment registry, ABI/topic validation, redundant RPC clients, stale-head detection, confirmation policy, customer deposit-status API, frontend transaction discovery, credited-event reversal, custody reconciliation, and operational alerting remain Milestone 3 blockers.
