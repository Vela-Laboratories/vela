# Authoritative ledger data model

## Scope

PostgreSQL is Vela's authoritative financial system of record. The matching engine proposes deterministic effects; the ledger validates and commits ownership changes before any success response or public event. All asset amounts are signed integer base units stored as PostgreSQL `NUMERIC`, never floating point.

The base schema lives at [`ledger/migrations/0001_authoritative_ledger.sql`](../../ledger/migrations/0001_authoritative_ledger.sql). Forward migration `0002_deposit_state_machine.sql` adds canonical case-insensitive event identity, database transition guards, and same-transaction history enforcement. Migration `0003_chain_indexer.sql` adds the immutable deployment registry, single-worker lease, monotonic finalized checkpoint, and halt evidence for the Base indexer. Migration `0004_deposit_reorg_recovery.sql` adds cyclic finalized-history audits, credited-deposit reversal references, and asset-wide financial halts. Migration `0005_withdrawal_reservations.sql` adds customer-nonce uniqueness, guarded withdrawal transitions, immutable identity fields, and mandatory same-transaction state history. Migration `0006_withdrawal_authorizations.sql` adds immutable, contract-bound approval envelopes and custody-signature evidence. Migration `0007_withdrawal_policy_controls.sql` adds versioned atomic-unit limits, immutable policy decisions, and a global withdrawal halt. The `ledger` Rust crate owns the posting transaction and SQLx migration runner.

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

`ledger_deposits`, `ledger_deposit_chain_events`, and `ledger_deposit_state_history` store the complete deposit projection, canonical chain identity, and append-only transition history. `(chain_id, lower(transaction_hash), log_index)` and the vault deposit ID are independently unique. The typed ledger service makes observation replay-safe, requires two-provider finalized evidence, and commits the final custody/customer journal movement atomically with `credited` state and history. See [Verified deposit ledger boundary](verified-deposit-ledger.md).

`ledger_withdrawals` and `ledger_withdrawal_state_history` store the current withdrawal projection, immutable transition history, reservation/completion/release journal references, destination, nonce, expiry, and broadcast identity. `Ledger::reserve_withdrawal` persists the request and moves customer ownership from `customer_available` to `customer_withdrawal_reserved` in one serializable transaction. The customer and request ID are replay-safe, and one customer nonce cannot authorize two different requests. A cancellation is allowed only before signing and journals an exactly-once release back to available funds.

`ledger_withdrawal_policy_limits` publishes immutable policy versions per asset, expressed only in atomic units. `Ledger::evaluate_withdrawal_policy` locks the active policy and reserved withdrawal in a serializable transaction, calculates the single, customer amount/count, and platform amount windows, incorporates a provider-neutral compliance disposition, and stores one immutable decision with exact observations and a SHA-256 evidence hash. The current reservation counts toward every applicable window. Rejected, review-required, and provider-unavailable decisions cannot authorize custody.

`ledger_global_withdrawal_halts` supplies an emergency stop across reservation, completion, approval, and signing. Activation is immediate and replay-safe. Resolution is the only permitted mutation and requires evidence plus three identities: the original activator, a different resolution initiator, and a different final resolver. Existing pre-signing reservations may still use the safe cancellation release path.

`ledger_withdrawal_authorizations` binds the request ID, chain, vault, customer, destination, asset code and on-chain token address, atomic amount, nonce, expiry, active policy version, and evaluator into a canonical Keccak-256 digest. Native ETH uses the zero address as its canonical asset address. PostgreSQL accepts it only when the same withdrawal has an approved decision under the currently effective asset policy and neither the global nor asset halt is active. `ledger_withdrawal_signatures` stores one immutable recoverable secp256k1 signature per authorization and repeats the halt check at signature storage. External sanctions/KYT screening, broadcast monitoring, completion, failure release, and finalized reconciliation remain separate work.

## Custody, reconciliation, and corrections

`ledger_custody_snapshots` and `ledger_custody_snapshot_balances` capture finalized vault evidence by chain, block, deployment, asset, and input hash. `ledger_reconciliation_results` records custody, liabilities, their exact difference, status, and evidence per asset. These records are immutable.

`ledger_administrative_corrections` binds a balanced journal transaction to its typed reason, evidence, incident, initiator, independent approver, and approval time. The ledger exposes a dedicated typed correction command, rejects case-insensitive self-approval, and prevents the generic posting path from creating an unaudited administrative adjustment. The command hashes all approval evidence and financial legs, then commits the journal, balance projection, outbox event, and immutable correction record in one serializable transaction. The operator-facing service must authenticate both actor references before it constructs this command.

## Ownership and verification

| Invariant | Runtime owner | Database owner | Current verification |
| --- | --- | --- | --- |
| Every journal transaction balances per asset | `ledger::Ledger` | Deferred constraint trigger | Direct unbalanced SQL commit test and generated multi-leg property test |
| Amounts are integer base units | `ledger::Ledger` | Numeric checks | Fractional command rejection test |
| Customer balances cannot be overspent | `ledger::Ledger` | Row locks and serializable transaction | Concurrent debit test |
| Retries cannot apply twice | `ledger::Ledger` | Unique idempotency key | Same-request and conflicting-request tests |
| Journal and evidence history is immutable | Ledger services | Mutation triggers | Journal update/delete test plus migration constraints |
| Balance projections equal journal history | `ledger::Ledger` | Atomic transaction | Projection verification test |
| Engine financial cache equals journal ownership after restart | API startup replay | Immutable journal sequence | PostgreSQL restart and stale-cache replacement test |
| Financial events follow commit | Outbox publisher | Transactional outbox foreign key | Outbox assertion in posting test |
| Deposits have one canonical event identity | Deposit ledger service and future Base indexer | Case-insensitive unique chain-event constraints | Observation retry and conflict integration tests; indexer remains pending |
| Finalized deposit credit is atomic and idempotent | Deposit ledger service | Journal, projection, deposit transaction FK, transition/history triggers | PostgreSQL finality, double-credit, balance, and transition-bypass tests |
| Contradicted credits cannot remain usable | Deposit indexer and ledger service | Asset-halt journal trigger, reversal FK, transition/history guards | Two-provider historical rewrite, complete reversal, spent-credit quarantine, blocked-operation, and retry tests |
| Administrative corrections require two people | Typed ledger correction command | Initiator/approver inequality | Self-approval, idempotency, bypass, and atomic-rollback tests |
| Concurrent withdrawals cannot exceed available ownership | Withdrawal reservation service | Serializable projection-row locks and nonnegative customer accounts | Concurrent PostgreSQL reservation test |
| Withdrawal retries cannot reserve twice | Withdrawal reservation service | Customer-nonce and journal idempotency uniqueness | Identical replay and conflicting replay tests |
| Pre-signing cancellation cannot create or destroy ownership | Withdrawal cancellation service | Balanced release journal and guarded transition/history triggers | Exactly-once cancellation and balance-restoration test |
| Custody cannot sign mutable or unapproved facts | Authorization and signer boundary | Immutable authorization digest, one signature per request, guarded state transitions | PostgreSQL binding, replay, mutation, and test-signer recovery tests |
| Withdrawal policy cannot be skipped or silently stale | Policy evaluator and authorization boundary | Active-version lookup, immutable decision evidence, halt guards | PostgreSQL limit, compliance, replay, bypass, and halt tests |

## Schema evolution

SQLx records applied migration versions in `_sqlx_migrations`. Applied files are immutable. Changes use forward-only migrations and an expand, migrate, verify, contract sequence. Production rehearsal, backup restoration, role grants, replication, and managed-provider provisioning remain separate deployment work and are not implied by the local schema.
