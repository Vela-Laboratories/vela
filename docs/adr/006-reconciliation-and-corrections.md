# ADR-006: Reconciliation, halt policy, and administrative corrections

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

Vela continuously reconciles each asset independently:

`confirmed vault assets = customer liabilities + fees payable + withdrawal reservations + documented operational liabilities`

The reconciler uses finalized chain state from two providers and a repeatable-read ledger snapshot. Results, inputs, deployment versions, timestamps, and hashes are stored immutably. A daily signed report is retained outside the engine host.

Any custody deficit, unknown outbound transfer, finalized-provider disagreement, or unexplained ledger delta immediately halts deposits, trading, and withdrawals for the affected asset. Signer or withdrawal-orchestrator failure halts withdrawals. Indexer failure halts credits. Ledger uncertainty halts all financial operations. Recovery requires a fresh successful reconciliation and two-person approval; services do not self-unhalt after a severe event.

Administrative corrections are balanced compensating journal transactions with a typed reason, evidence reference, incident ID, initiator, independent approver, and idempotency key. The initiator cannot approve their own correction. Neither admin APIs nor database operators may edit historical journal entries.

## Consequences

- Readiness reports dependency health by operation and asset rather than a single green status.
- Customer-visible status distinguishes unavailable operations from system liveness.
- Reconciliation failures page on-call and open an incident automatically.
