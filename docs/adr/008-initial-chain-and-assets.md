# ADR-008: Initial chain and asset representations

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

The first production chain is Base Mainnet, chain ID `8453`. Staging uses Base Sepolia, chain ID `84532`, with completely separate contracts, databases, keys, domains, and account namespaces.

The only launch market is `ETH-USDC`. ETH is Base's native gas asset and is accounted for in wei with 18 decimals. USDC is Circle's native Base USDC at `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913`, accounted for in its on-chain base units with 6 decimals. Bridged USDbC, WETH, fee-on-transfer tokens, rebasing tokens, and all other representations are rejected at launch.

The deployment registry pins chain ID, genesis identifier, vault address, runtime bytecode hash, asset address, decimals, deposit-event ABI hash, and activation block. Services refuse to start if runtime discovery disagrees with the signed registry. Customer interfaces display the chain and contract identity before any transaction.

Base was selected for EVM compatibility, native Circle USDC, lower customer transaction costs than Ethereum L1, and standard `safe`/`finalized` RPC semantics. This is a product and engineering decision, not legal approval. Redundant production RPC contracts and provider vendors are selected during implementation.

## Consequences

- The current Ethereum Sepolia deployment remains a demo and has no migration path into production balances.
- Native ETH deposits require a payable vault path; ERC-20 logic is limited to native USDC.
- Adding an asset, wrapper, bridge, or chain requires a new ADR, legal review, custody support, reconciliation support, and external security review.

## References

- [Base RPC networks and chain IDs](https://docs.base.org/base-chain/api-reference/rpc-overview)
- [Circle USDC contract addresses](https://developers.circle.com/stablecoins/usdc-contract-addresses)
