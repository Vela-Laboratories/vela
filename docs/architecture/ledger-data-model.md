# Authoritative ledger data model

## Scope

PostgreSQL is Vela's authoritative financial system of record. The matching engine proposes deterministic effects; the ledger validates and commits ownership changes before any success response or public event. All asset amounts are signed integer base units stored as PostgreSQL `NUMERIC`, never floating point.

The first migration lives at [`ledger/migrations/0001_authoritative_ledger.sql`](../../ledger/migrations/0001_authoritative_ledger.sql). The `ledger` Rust crate owns the posting transaction and SQLx migration runner.

## Accounts

Every account is scoped to exactly one asset and one owner reference. `ledger_accounts` supports these account categories:

| Category | Purpose | Normally nonnegative |
| --- | --- | --- |
| `customer_available` | Finalized ownership available to trade or withdraw | Yes |
| `customer_locked` | Funds reserved for resting orders | Yes |
| `customer_pending` | Observed or quarantined funds unavailable for use | Yes |
| `customer_withdrawal_reserved` | Approved withdrawal funds awaiting final chain outcome | Yes |
| `custody_vault` | Assets confirmed in the omnibus vault | Policy dependent |
| `custody_withdrawn` | Completed outbound custody movements | Policy dependent |
| `fee_revenue` | Explicit exchange fee accounting | Policy dependent |
| `rebate_expense` | Explicit maker-rebate accounting | Policy dependent |
| `operational_adjustment` | Counter-account for approved corrections | Yes, by explicit configuration only |

`ledger_account_balances` is a projection with an account version. Posting locks affected projection rows in UUID order, verifies nonnegative-account constraints, writes journal legs, advances versions, and inserts an outbox event in one `SERIALIZABLE` transaction.

## Immutable journal

`ledger_journal_transactions` contains the operation type, globally unique idempotency key, 32-byte request hash, external reference, metadata, and creation time. `ledger_journal_legs` contains signed account and asset amounts. Database constraints enforce integer, nonzero amounts and ensure each leg's asset matches its account.

A deferred PostgreSQL constraint trigger rejects a transaction at commit unless it has at least two legs and sums to zero independently for every asset. Mutation triggers reject updates and deletes of journal headers and legs. Corrections are new balanced transactions.

The request hash binds an idempotency key to its original command. An identical retry returns the original transaction; reuse for a different request fails without applying either request twice.

## Deposits and withdrawals

`ledger_deposits`, `ledger_deposit_chain_events`, and `ledger_deposit_state_history` store the complete deposit projection, canonical chain identity, and append-only transition history. `(chain_id, transaction_hash, log_index)` and the vault deposit ID are independently unique.

`ledger_withdrawals` and `ledger_withdrawal_state_history` store the current withdrawal projection, immutable transition history, reservation/completion/release journal references, destination, nonce, expiry, and broadcast identity. The allowed state values match ADR-003 and ADR-004.

## Custody, reconciliation, and corrections

`ledger_custody_snapshots` and `ledger_custody_snapshot_balances` capture finalized vault evidence by chain, block, deployment, asset, and input hash. `ledger_reconciliation_results` records custody, liabilities, their exact difference, status, and evidence per asset. These records are immutable.

`ledger_administrative_corrections` binds a balanced journal transaction to its typed reason, evidence, incident, initiator, independent approver, and approval time. The database rejects self-approval. The future administrative service must verify authorization and dual approval before inserting either record.

## Ownership and verification

| Invariant | Runtime owner | Database owner | Current verification |
| --- | --- | --- | --- |
| Every journal transaction balances per asset | `ledger::Ledger` | Deferred constraint trigger | Direct unbalanced SQL commit test |
| Amounts are integer base units | `ledger::Ledger` | Numeric checks | Fractional command rejection test |
| Customer balances cannot be overspent | `ledger::Ledger` | Row locks and serializable transaction | Concurrent debit test |
| Retries cannot apply twice | `ledger::Ledger` | Unique idempotency key | Same-request and conflicting-request tests |
| Journal and evidence history is immutable | Ledger services | Mutation triggers | Journal update/delete test plus migration constraints |
| Balance projections equal journal history | `ledger::Ledger` | Atomic transaction | Projection verification test |
| Financial events follow commit | Outbox publisher | Transactional outbox foreign key | Outbox assertion in posting test |
| Deposits have one canonical event identity | Future Base indexer | Unique chain-event constraints | Migration constraint; indexer integration test pending |
| Administrative corrections require two people | Future admin service | Initiator/approver inequality | Service authorization test pending |

## Schema evolution

SQLx records applied migration versions in `_sqlx_migrations`. Applied files are immutable. Changes use forward-only migrations and an expand, migrate, verify, contract sequence. Production rehearsal, backup restoration, role grants, replication, and managed-provider provisioning remain separate deployment work and are not implied by the local schema.
