# Withdrawal execution observer operations

## Scope

EX-026 adds a source- and test-only withdrawal execution boundary. It does not deploy the V2 settlement contract, configure an RPC provider, run a withdrawal worker, broadcast a transaction, or move real funds. The provider-neutral observer is disabled by default.

The V2 contract accepts a permissionlessly relayed EIP-712 authorization. The signed payload binds the request ID, customer, destination, asset, atomic amount, customer nonce, deadline, policy hash, and approver hash to the settlement contract and chain. Any account may pay gas to relay it, but the relayer cannot change the signed withdrawal facts.

## Deployment registration

Do not authorize or observe V2 withdrawals until the deployed runtime bytecode and event ABI have been externally reviewed. With the migrator credential, register the deployment in `ledger_chain_deployments`, then add exactly one matching row to `ledger_withdrawal_contract_capabilities`.

The capability row pins:

- authorization version `eip712_v2`;
- event signature `WithdrawalExecuted(bytes16,address,address,address,uint256,uint256,uint256,bytes32,bytes32)`;
- the corresponding lowercase event topic;
- an operator identity and non-sensitive review evidence.

The database rejects executable authorizations for deployments without this capability. Registration is immutable. A replacement contract requires a new deployment record instead of editing historical evidence.

## Confirmation policy

Publish a reviewed `ledger_withdrawal_confirmation_policies` row for each enabled V2 deployment. Policies are immutable and require two distinct providers, finalized evidence, and a positive minimum-confirmation count. A new threshold is a new version with a later effective time.

Do not treat a latest-head confirmation count as finality. The orchestration worker must obtain matching normalized evidence from two independently configured RPC origins and preserve provider references outside generic customer-visible metadata.

## Broadcast attempts and replacement

`Ledger::record_withdrawal_broadcast_attempt` stores provider acceptance of an exact executable transaction. Each immutable attempt records its transaction hash, sender, sender nonce, destination vault, calldata hash, provider reference, submitter, and submission time.

The first accepted transaction moves the projection to `broadcast`. A replacement must identify the immediately preceding attempt and moves it to `replaced`. An identical retry returns the existing attempt; conflicting reuse fails. Recording an attempt is evidence storage only. This repository does not contain an enabled runtime broadcaster.

`Ledger::record_dropped_withdrawal_attempt` accepts two-provider evidence only for the currently tracked transaction, persists that observation immutably, and moves `broadcast` or `replaced` to `dropped`. An older replaced transaction cannot mark a newer live attempt dropped. A dropped observation never releases funds; the authorization remains executable until its deadline and must follow the permanent non-execution procedure below.

The API's `WithdrawalChainObservationService` validates provider identity and exact transaction hash, sender, vault, and calldata hash. Its default construction uses `DisabledWithdrawalChainObserver`. The deterministic observer exists only in tests.

## Finalized execution

`Ledger::confirm_withdrawal_execution` accepts only an observation that matches the registered V2 deployment, immutable withdrawal identity, active confirmation policy, and two distinct providers. Finalized `WithdrawalExecuted` evidence must match the request ID, customer, destination, asset address, amount, and nonce.

In one serializable transaction, the ledger:

1. stores the immutable chain observation and its SHA-256 evidence hash;
2. journals the exact amount from `customer_withdrawal_reserved` to `custody_withdrawn`;
3. stores the one-to-one execution record;
4. moves the withdrawal to `confirmed` with immutable state history.

The database rejects a direct `confirmed` transition without matching execution evidence and completion journal identity. Retries are exactly once; conflicting evidence fails.

Finalized execution must be recorded even while a withdrawal halt is active. The halt prevents new exposure; it must not hide custody movement that already occurred or leave the authoritative ledger inconsistent with chain state.

## Expiry and permanent non-execution

A pending, dropped, or reverted transaction is ambiguous while its authorization can still execute. Never release its reservation merely because one transaction is missing or failed.

`Ledger::release_expired_withdrawal` may restore funds only after the V2 authorization deadline and only from finalized, two-provider evidence proving all of the following:

- the request ID is unused;
- the customer nonce is unused;
- no matching execution event exists;
- every recorded broadcast attempt is terminal;
- the configured confirmation policy is satisfied.

The ledger stores that evidence, journals the exact amount from `customer_withdrawal_reserved` back to `customer_available`, stores a one-to-one failure-release record, and moves the withdrawal to `broadcast_failed` in the same serializable transaction. The database rejects the terminal state without the matching release journal and observation. Identical retries do not release twice.

## Required work before deployment

- Externally audit the V2 custody contract and freeze the reviewed commit.
- Deploy and register a V2 testnet contract with pinned runtime bytecode.
- Configure two independent RPC providers and an authenticated, leased observer worker.
- Implement and operate a controlled broadcaster or a reliable customer-relay discovery mechanism.
- Add continuous replacement, dropped-transaction, and historical-finality re-auditing.
- Reconcile finalized custody movement against ledger liabilities before reopening affected operations.
- Run end-to-end ETH and USDC tests against the deployed contract before considering real funds.
