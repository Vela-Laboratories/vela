# ADR-002: Custody contract model

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

The first release will use one audited omnibus custody vault on Base. The vault holds allowlisted assets while the authoritative off-chain ledger records customer beneficial ownership. It will not maintain per-user trading balances on-chain.

Deposits emit an event containing depositor, credited account identifier, asset, and actual received amount. ERC-20 deposits calculate the vault balance increase and reject unsupported, rebasing, and fee-on-transfer tokens. Native ETH uses a dedicated payable deposit entry point.

Withdrawals require a unique ledger request ID and a policy-authorized threshold signature bound to chain ID, vault address, destination, asset, amount, expiry, and nonce. Executed request IDs and nonces are permanently consumed. Pausing can stop deposits or withdrawals independently, but cannot alter ownership records.

Contract administration uses separate owner, guardian, and withdrawal-authorizer roles under hardware-backed threshold control. Upgrades require a timelock, independent review, and a tested migration path. The guardian may pause but may not transfer customer assets. Migration may move assets only to an allowlisted successor vault under the same liability reconciliation process.

## Consequences

- The existing per-user contract accounting is replaced rather than extended.
- Solvency depends on continuous reconciliation between omnibus custody and ledger liabilities.
- Contract events are evidence for the indexer, not authority to credit until finality and ledger commit.
- Proof-based withdrawals and forced inclusion are Stage 2 additions, not launch assumptions.
