# Vela contracts - external audit scope

**Version:** v3.0 (V3 omnibus custody boundary)
**Contact:** engineering@monolithsystematic.com
**Repository:** https://github.com/Monolith-Investments-LP/vela

This scope is for the external audit required before any real-funds deployment. The repository remains testnet-only and no deployment or audit completion is implied by this document.

## Scope

Only [`VelaSettlement.sol`](src/VelaSettlement.sol) is in scope for the initial custody deployment. It is Solidity `0.8.28`, Cancun EVM, with `optimizer_runs = 200`. The contract is an immutable omnibus ETH/ERC20 vault.

The initial boundary includes:

- owner-managed asset allowlisting and exact ERC20 receipt checks;
- deposit events carrying a unique derived deposit ID and opaque off-chain ledger account ID, with no on-chain per-user balances;
- relayed EIP-712 withdrawals binding request ID, destination, asset, amount, nonce, deadline, policy hash, approver hash, chain ID, and vault address;
- independent deposit and withdrawal pauses, with owner-only resumption;
- owner, guardian, and operator separation with delayed operator rotation;
- successor-vault migration requiring both pauses, owner proposal, independent guardian approval, a seven-day timelock, exact operator authorization, and a liability snapshot hash.

The following are explicitly out of scope for the initial custody deployment: `InsuranceFund.sol`, `PerpEngine.sol`, `VLPVault.sol`, and `SequencerRegistry.sol`; the Rust matching engine; frontend code; API OpenAPI documentation; and upstream libraries in `contracts/lib/`.

## Threat model

### Compromised operator

The operator can authorize a withdrawal payload, subject to the signature and vault checks. It cannot pause or unpause the vault, change roles, or replay a request ID or nonce. EIP-712 domain separation prevents authorization for a different chain or vault from being reused.

### Compromised owner

The owner can configure assets, pause state, guardian, operator rotation, and migration proposals. It cannot produce an operator signature. Migration still requires independent guardian approval, the seven-day delay, an exact liability snapshot, and operator authorization.

### Compromised guardian

The guardian can pause deposits and withdrawals and approve a proposed successor. It cannot unpause, rotate roles, or transfer customer assets. There is no self-service emergency-exit bypass; recovery uses the guarded migration procedure.

### Unavailable or malicious L2 sequencer

The configured sequencer uptime feed is checked fail-closed for withdrawal and migration paths, including the configured grace period after recovery.

### Malicious ERC20 and reentrancy

Supported ERC20 deposits measure the vault balance delta and revert unless it equals the requested amount. SafeERC20 and non-reentrancy guards protect fund-moving paths. Asset review remains an operational requirement before allowlisting.

## Invariants auditors should verify

1. Deposits emit a unique deposit ID and opaque account ID without creating an on-chain customer balance.
2. Unsupported and fee-on-transfer deposits revert atomically; exact received amounts are the only accepted ERC20 custody evidence.
3. Withdrawal signatures bind every value and policy field to the EIP-712 domain, and request IDs and nonces are permanently consumed.
4. Pauses are independent and only the owner can resume them.
5. Migration cannot execute without both pauses, owner proposal, guardian approval, the seven-day timelock, exact operator authorization, a compatible successor, and an exact liability snapshot amount.
6. Migration verifies the successor's received amount before emitting its completion event.

## Known limitations and non-goals

1. The vault is immutable. A post-deployment defect requires pausing, reconciliation, and guarded migration to a reviewed successor. There is no customer self-service emergency-exit path.
2. Ledger reservation, policy evaluation, signing, broadcast, finality, reconciliation, and customer authorization remain off-chain launch gates.
3. Only the initial ETH/USDC custody boundary is proposed; additional assets, chains, proof-based settlement, and forced inclusion require a separate design and review.

## Required audit deliverables

1. Findings report in Markdown and PDF with severity, location, impact, likelihood, and remediation guidance.
2. Machine-readable findings JSON.
3. Signed final SHA of the audited commit.
4. Any added invariants or regression tests.

The external audit, finding remediation, audited-commit freeze, deployment review, and real-funds approval remain outstanding gates.

*Last updated 2026-09-19 alongside the V3 custody-boundary redesign.*
