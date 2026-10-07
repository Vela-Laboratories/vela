# ADR-002: Custody contract model

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

The first release will use one audited V3 omnibus custody vault on Base. The vault holds allowlisted assets while the authoritative off-chain ledger records customer beneficial ownership. It will not maintain per-user trading balances on-chain, and it will not expose a customer self-service emergency-exit path.

Deposits emit an event containing a derived unique deposit ID, depositor, opaque account identifier, asset, and actual received amount. For the initial self-deposit flow, the indexed depositor wallet is the ledger owner reference. The 32-byte account identifier is an audit marker, not an on-chain balance owner or a replacement for the wallet identity used by trading and customer APIs. ERC-20 deposits calculate the vault balance increase and reject unsupported, rebasing, and fee-on-transfer tokens. Native ETH uses a dedicated payable deposit entry point.

Withdrawals are relayed by any caller but require a policy-authorized EIP-712 signature bound to chain ID, vault address, unique ledger request ID, destination, asset, amount, expiry, nonce, policy hash, and approver hash. Executed request IDs and nonces are permanently consumed. Pausing can stop deposits or withdrawals independently, but cannot alter ownership records.

Contract administration uses separate owner, guardian, and withdrawal-operator roles under hardware-backed threshold control. The guardian may pause but may not transfer customer assets. The immutable vault has no upgrade function. Migration may move assets only to a compatible successor vault after both pause domains are active, owner proposal, independent guardian approval, a seven-day timelock, exact operator authorization, and a liability snapshot hash.

## Consequences

- The existing per-user contract accounting is replaced rather than extended.
- Solvency depends on continuous reconciliation between omnibus custody and ledger liabilities.
- Contract events are evidence for the indexer, not authority to credit until finality and ledger commit.
- No on-chain per-user balances or self-service emergency exit are launch assumptions.
- Proof-based settlement and forced inclusion are Stage 2 additions, not launch assumptions.
