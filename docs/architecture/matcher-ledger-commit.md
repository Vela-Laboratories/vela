# Matcher-ledger commit boundary

## Purpose

The matching engine is an execution service and cache. PostgreSQL is the authoritative owner of customer funds. A financially effective order result must therefore be durable in the ledger before Vela publishes the resulting balances or acknowledges success.

## Commit protocol

When `DATABASE_URL` configures the ledger, the sharded dispatcher processes each batch as follows:

1. Lock the committed customer state and market shards so concurrent readers and writers cannot observe or overwrite a proposal.
2. Clone the committed engine state into a private proposal.
3. Execute the complete batch against that proposal and capture responses internally.
4. Diff customer available and locked balances, then derive fee revenue, maker-rebate expense, and referral payouts from the fills.
5. Verify that the complete effect balances to zero independently for every asset.
6. Resolve every required ledger account and convert engine units into the asset's configured integer base units without rounding.
7. Post one serializable, idempotent journal transaction containing all legs and the transactional outbox event.
8. Publish the proposed engine state and release responses only after the database commit succeeds.

Validation, account resolution, decimal conversion, or database failure discards the proposal. The caller receives a generic internal error, committed engine state remains unchanged, and `vela_financial_commit_failures_total` increments. Internal logs retain the diagnostic without exposing database details to the customer.

Development and `testnet-demo` can run without a configured ledger. That path remains explicitly non-authoritative and must not be used to accept real funds.

## Journal coverage

The dispatcher journals the full net balance effect rather than reconstructing ownership from a fill alone. This covers:

- available-to-locked order reservations;
- locked-to-available cancellation and unused-reservation releases;
- atomic base and quote ownership transfers;
- taker and positive maker fees in `fee_revenue`;
- negative maker fees in `rebate_expense`;
- referral payouts funded from fee revenue.

The journal metadata retains the complete effect set and engine responses, and references each fill by market, maker order, taker order, and engine timestamp. Including order and cancellation identities prevents equal-sized reservations or releases from sharing a key. A SHA-256 hash of the canonical serialized effect supplies the matcher idempotency key and request hash.

## Account provisioning

Account lookup is exact and fails closed. Before an order can affect an account, the database must contain the matching asset and account tuple:

| Owner | Owner reference | Account types |
| --- | --- | --- |
| Customer | Lowercase `UserId::to_hex()` | `customer_available`, `customer_locked` |
| Platform | `platform:vela` | `fee_revenue`, `rebate_expense` |

Only accounts with a nonzero effect are required for a particular transaction. Customer accounts remain nonnegative. Platform policy determines whether fee and rebate accounts may carry signed balances.

## Amount conversion

The engine represents price and quantity with eight decimal places. Ledger assets use their actual configured decimal count. Conversion multiplies for assets with more than eight decimals and divides exactly for assets with fewer decimals. A value that cannot be represented exactly is rejected instead of rounded. For six-decimal USDC, an engine amount must therefore be divisible by 100.

## Verification and limitations

Automated tests prove that responses and committed shard state remain unavailable while a financial commit is blocked, failed commits roll back the proposal, missing accounts fail closed, and an ETH/USDC fill moves base, quote, taker fee, and maker rebate atomically in PostgreSQL.

This chapter does not complete deposit indexing, withdrawal orchestration, journal replay, custody reconciliation, or managed database disaster recovery. Those remain explicit launch blockers in the engineering plan and launch checklist.
