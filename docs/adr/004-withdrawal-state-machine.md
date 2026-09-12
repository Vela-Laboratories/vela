# ADR-004: Withdrawal state machine

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

A withdrawal follows `requested -> reserved -> approved -> signing -> signed -> broadcast -> confirmed`. Exceptional states are `rejected`, `cancelled`, `signing_failed`, `broadcast_failed`, `replaced`, `dropped`, and `reversed`.

The request is durably stored before any signature is requested. In one serializable ledger transaction, Vela checks authenticated ownership, available and locked balances, compliance holds, risk limits, asset status, and destination policy, then moves funds from `customer_available` to `customer_withdrawal_reserved`. This commit is the earliest point at which approval may begin.

The signer accepts only an immutable approved payload containing request ID, ledger transaction ID, chain ID, vault, destination, asset, amount, nonce, expiry, and policy version. It is threshold MPC or HSM backed, has no general-purpose transfer interface, and rejects duplicate request IDs.

Broadcast is controlled by Vela and tracked through replacement and confirmation. Final on-chain execution atomically moves the reservation to `custody_withdrawn`. A permanently failed request moves the exact reservation back to available once, using a unique compensating journal key. Ambiguous transactions remain reserved and escalate; they are never automatically refunded while they may still execute.

## Consequences

- Customer-side broadcast and the current independent signature endpoint are removed from production.
- A global and per-asset kill switch can block new approvals without corrupting existing state.
- Every state transition is customer-visible and auditable.
