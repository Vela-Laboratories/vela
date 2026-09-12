# Vela Architecture Decision Records

These records define the financial architecture for the first consumer release. An accepted ADR is the default implementation decision until a superseding ADR is reviewed and merged.

| ADR | Decision | Status |
|---|---|---|
| [ADR-001](001-authoritative-ledger.md) | PostgreSQL double-entry ledger is authoritative | Accepted |
| [ADR-002](002-custody-contract.md) | Omnibus on-chain vault with off-chain ownership ledger | Accepted |
| [ADR-003](003-deposit-state-machine.md) | Finalized, idempotent Base deposit indexing | Accepted |
| [ADR-004](004-withdrawal-state-machine.md) | Reserve before approval, signing, or broadcast | Accepted |
| [ADR-005](005-matcher-journal-boundary.md) | Ledger transaction is the trade commit boundary | Accepted |
| [ADR-006](006-reconciliation-and-corrections.md) | Asset-scoped reconciliation, automatic halts, dual-control corrections | Accepted |
| [ADR-007](007-recovery-and-fencing.md) | Multi-AZ ledger, fenced single writer, zero acknowledged-operation RPO | Accepted |
| [ADR-008](008-initial-chain-and-assets.md) | Base, native ETH, and native USDC only | Accepted |

Implementation work must preserve the invariants in the [consumer exchange engineering plan](../consumer-exchange-engineering-plan.md). Legal approval, vendor diligence, threat modeling, and external audits remain launch gates; these ADRs do not satisfy them.
