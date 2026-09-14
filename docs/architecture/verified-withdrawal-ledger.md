# Authoritative withdrawal reservation boundary

## Scope

EX-023 establishes the first safe withdrawal boundary for the private testnet build. It does not claim that automated custody signing, broadcast, confirmation monitoring, or reconciliation is complete.

`POST /withdrawals` accepts an EIP-191 wallet signature over the request ID, customer, destination, asset, exact eight-decimal request amount, and nonce. ETH and USDC use the same endpoint and ledger command. The API converts the request amount to the configured ledger asset's base units without floating point.

When PostgreSQL is authoritative, the API never dispatches the withdrawal to the matching engine. `Ledger::reserve_withdrawal` instead performs one serializable transaction that:

- verifies the configured asset, customer available account, and single custody-vault account;
- inserts the immutable withdrawal identity and initial history;
- journals equal and opposite legs from `customer_available` to `customer_withdrawal_reserved`;
- updates the withdrawal from `requested` to `reserved` with its journal reference;
- writes the second immutable history entry and transactional outbox event;
- commits before the API acknowledges success.

The customer nonce and request ID are unique. An identical retry returns the original record and journal transaction. Reusing either identity for different request fields fails without moving funds. Serializable balance locks prevent concurrent withdrawals from exceeding available ownership.

## Cancellation and signing boundary

`POST /withdrawals/{id}/cancel` requires a fresh customer signature. Only `reserved` requests can be cancelled. Cancellation journals the exact reverse movement and records `cancelled` in the same transaction; retries return the original release rather than crediting twice.

The legacy `/withdrawal-signature` route fails closed whenever the authoritative ledger is configured. This prevents the environment-held demo signer from releasing custody assets without consuming a durable reservation.

EX-024 adds a provider-neutral `CustodySigner` boundary for a dedicated custody worker. Approval stores one immutable authorization envelope that binds the withdrawal request ID, chain, vault contract, customer, destination, asset code and on-chain token address, atomic amount, nonce, expiry, policy version, and approver. Its Keccak-256 digest is the only signing payload exposed to an adapter. The boundary recovers every returned signature against the adapter's configured secp256k1 public key before storing it. PostgreSQL refuses `approved`, `signing`, and `signed` transitions unless their matching authorization and signature evidence already exist, and both evidence tables are append-only.

The default signer is disabled. A deterministic secp256k1 implementation exists only inside Rust tests, so this chapter adds no deployable private key, vendor account, or cost. Adapter failures leave the amount reserved and retryable rather than pretending a transfer occurred. A production HSM or MPC adapter, risk/compliance approval source, broadcast worker, permanent-failure release, and confirmation reconciliation remain required before real funds.

EX-025 puts a durable risk-policy boundary in front of that signer. Immutable per-asset policy versions define a single-withdrawal maximum, customer amount and count windows, and a platform amount window in exact atomic units. A serializable evaluation stores the limits, observed totals, halt state, provider-neutral compliance disposition, reason codes, and evidence hash. PostgreSQL will not accept the authorization unless that decision is approved, uses the currently effective policy, and names the same evaluator as the authorization approver.

The global withdrawal halt blocks new reservations, approvals, signing transitions, and signature storage while leaving safe cancellation and finalized expiry releases available. It can be activated immediately, but resolution needs evidence and three distinct operator identities. A halt does not suppress a matching finalized execution that already moved custody; the completion journal must still reconcile the ledger to chain state. Asset-wide reconciliation halts remain independently enforced. No external sanctions or blockchain-analytics service is configured by this free implementation: a provider can later supply the neutral disposition contract, and `review` or `unavailable` remains fail-closed.

## V2 execution and finality boundary

EX-026 replaces the contract's legacy caller-bound authorization source with a V2 EIP-712 withdrawal. Its domain binds the signature to one chain and settlement deployment. Its struct binds the request ID, customer, destination, asset, atomic amount, customer nonce, deadline, policy hash, and approver hash. Any account may relay the signed authorization, but it cannot substitute the customer, recipient, asset, amount, policy, or approver. The contract consumes the request ID and per-customer nonce before transferring ETH or ERC20 and emits all signed facts in `WithdrawalExecuted`.

Migration `0008_withdrawal_execution.sql` admits executable authorizations only for registered V2-capable deployments. It stores immutable broadcast attempts, replacement links, confirmation policies, chain observations, finalized executions, and permanent-failure releases. `Ledger::confirm_withdrawal_execution` requires matching two-provider finalized event evidence and atomically moves ownership from `customer_withdrawal_reserved` to `custody_withdrawn`. `Ledger::release_expired_withdrawal` restores the reservation exactly once only after finalized evidence proves the V2 deadline passed, both on-chain replay guards remain unused, no execution event exists, and every attempt is terminal.

The provider-neutral chain observer validates an observed transaction against its expected hash, sender, vault, and calldata hash. It is disabled by default, and its deterministic implementation is test-only. This chapter does not deploy the V2 contract, configure a real RPC provider, run a broadcaster or observer worker, or move funds. See [Withdrawal execution observer operations](../operations/withdrawal-observer.md).

## Customer status

`GET /account/{address}/withdrawals` reads only PostgreSQL and returns `503` when the authoritative ledger is absent. The frontend preflights this route before requesting a wallet signature, polls it for exact status, explains whether funds are reserved or require intervention, and allows a safe pre-signing cancellation. It makes no claim that a reservation is an on-chain transfer.
