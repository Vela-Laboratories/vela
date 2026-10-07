# Withdrawal execution observer operations

## Scope

EX-026 adds a source- and test-only withdrawal execution boundary for the V3 `VelaSettlement` omnibus vault. It does not deploy the V3 settlement contract, configure an RPC provider, run a withdrawal worker, broadcast a transaction, or move real funds. The provider-neutral observer is disabled by default.

The V3 contract accepts a permissionlessly relayed EIP-712 authorization. The signed payload binds the request ID, destination, asset, atomic amount, nonce, deadline, policy hash, and approver hash to the settlement contract and chain. Any account may pay gas to relay it, but the relayer cannot change the signed withdrawal facts.

## Deployment registration

Do not authorize or observe V3 withdrawals until the deployed runtime bytecode and event ABI have been externally reviewed. With the migrator credential, register the deployment in `ledger_chain_deployments`, then add exactly one matching row to `ledger_withdrawal_contract_capabilities`.

The capability row pins:

- authorization version `eip712_v3`;
- event signature `WithdrawalExecuted(bytes16,address,address,uint256,uint256,uint256,bytes32,bytes32)`;
- the corresponding lowercase event topic;
- an operator identity and non-sensitive review evidence.

The database rejects executable authorizations for deployments without this capability. Registration is immutable. A replacement contract requires a new deployment record instead of editing historical evidence.

## Confirmation policy

Publish a reviewed `ledger_withdrawal_confirmation_policies` row for each enabled V3 deployment. Policies are immutable and require two distinct providers, finalized evidence, and a positive minimum-confirmation count. A new threshold is a new version with a later effective time.

Do not treat a latest-head confirmation count as finality. The orchestration worker must obtain matching normalized evidence from two independently configured RPC origins and preserve provider references outside generic customer-visible metadata.

## Broadcast attempts and replacement

Schema version 17 adds `ledger_withdrawal_broadcast_queue`. Storing an approved
Base Sepolia V3 custody signature inserts exactly one immutable queue record in
the same ledger transaction. A failed queue insertion rolls back the signature
and state transition. The queue references the withdrawal, authorization, and
signature; it contains no relayer key or unsigned transaction and does not
submit anything. Legacy signed authorizations are not silently backfilled into
this queue. Operators must review them before any separate relay process acts.

`withdrawal-worker::relayer_policy` now exposes a deterministic, separately
tested decision over two provider quotes. It requires matching pending nonces,
uses the higher base and priority fee quotes, checks arithmetic overflow, and
rejects gas or total-cost ceilings. It has no runtime caller yet. A future
relayer must persist an exclusive nonce reservation and exact fee decision,
then durably record the signed transaction and uncertain submission outcomes
before this policy can close D-017 or enable broadcast.

`Ledger::record_withdrawal_broadcast_attempt` stores provider acceptance of the first exact executable transaction. Each immutable attempt records its transaction hash, sender, sender nonce, destination vault, calldata hash, provider reference, submitter, and submission time.

The first accepted transaction moves the projection to `broadcast`. Replacements use the separate `Ledger::record_observed_withdrawal_replacement` boundary. A supplied replacement hash must be observed independently by both providers with the same sender, Ethereum account nonce, vault destination, and authorization calldata hash as the latest attempt. The replacement and its immutable two-provider evidence commit atomically; a deferred database constraint rejects any replacement row without that evidence. An identical retry returns the existing attempt and conflicting evidence fails. Recording an attempt is evidence storage only. This repository does not contain an enabled runtime broadcaster.

Standard Ethereum JSON-RPC cannot discover an arbitrary pending same-nonce replacement from the predecessor hash. EX-028 therefore exposes a typed ingestion boundary for a trusted relay, transaction hash feed, or separately operated finalized-block scanner to supply the candidate hash. The worker verifies the candidate rather than trusting the source. This is not a claim of general mempool discovery, and continuous automatic replacement discovery remains deployment work.

`Ledger::record_dropped_withdrawal_attempt` accepts two-provider evidence only for the currently tracked transaction, persists that observation immutably, and moves `broadcast` or `replaced` to `dropped`. An older replaced transaction cannot mark a newer live attempt dropped. A dropped observation never releases funds; the authorization remains executable until its deadline and must follow the permanent non-execution procedure below.

The API's `WithdrawalChainObservationService` validates provider identity and exact transaction hash, sender, vault, and calldata hash. Its default construction uses `DisabledWithdrawalChainObserver`. The deterministic observer exists only in tests.

## Finalized execution

`Ledger::confirm_withdrawal_execution` accepts only an observation that matches the registered V3 deployment, immutable withdrawal identity, active confirmation policy, and two distinct providers. Finalized `WithdrawalExecuted` evidence must match the request ID, destination, asset address, amount, and nonce.

In one serializable transaction, the ledger:

1. stores the immutable chain observation and its SHA-256 evidence hash;
2. journals the exact amount from `customer_withdrawal_reserved` to `custody_withdrawn`;
3. stores the one-to-one execution record;
4. moves the withdrawal to `confirmed` with immutable state history.

The database rejects a direct `confirmed` transition without matching execution evidence and completion journal identity. Retries are exactly once; conflicting evidence fails.

Finalized execution must be recorded even while a withdrawal halt is active. The halt prevents new exposure; it must not hide custody movement that already occurred or leave the authoritative ledger inconsistent with chain state.

## Expiry and permanent non-execution

A pending, dropped, or reverted transaction is ambiguous while its authorization can still execute. Never release its reservation merely because one transaction is missing or failed.

EX-028 persists an exact finalized status-0 receipt as `finalized_revert`, tied one-to-one to the recorded attempt and protected by the immutable evidence triggers. Both providers must agree on the exact transaction facts, canonical receipt block, finalized checkpoint, and confirmation policy. A revert makes that attempt terminal, but does not itself change balances or release the reservation.

`Ledger::release_expired_withdrawal` may restore funds only after the V3 authorization deadline and only from finalized, two-provider evidence proving all of the following:

- the request ID is unused;
- the customer nonce is unused;
- no matching execution event exists;
- every recorded broadcast attempt is terminal;
- the configured confirmation policy is satisfied.

The ledger stores that evidence, journals the exact amount from `customer_withdrawal_reserved` back to `customer_available`, stores a one-to-one failure-release record, and moves the withdrawal to `broadcast_failed` in the same serializable transaction. The database rejects the terminal state without the matching release journal and observation. Identical retries do not release twice.

## Required work before deployment

- Externally audit the V3 custody contract and freeze the reviewed commit.
- Deploy and register a V3 testnet contract with pinned runtime bytecode.
- Configure two independent RPC providers and an authenticated, leased observer worker.
- Implement and operate a controlled broadcaster or a reliable source of replacement hashes for the verified ingestion boundary.
- Add continuous replacement, dropped-transaction, and historical-finality re-auditing.
- Reconcile finalized custody movement against ledger liabilities before reopening affected operations.
- Run end-to-end ETH and USDC tests against the deployed contract before considering real funds.

## EX-027 runtime worker

The `withdrawal-worker` crate is an opt-in, fail-closed V3 observer and ledger
reconciliation process. It starts only when
`VELA_WITHDRAWAL_WORKER_ENABLED=true`. This flag does not enable transaction
broadcasting: the production broadcaster remains `DisabledBroadcaster` and
holds no private key. The worker can monitor immutable attempts that another
reviewed relay path has already recorded.

Required settings are `DATABASE_URL`, `VELA_WITHDRAWAL_DEPLOYMENT_ID`, two
distinct provider names, and two HTTPS RPC URLs in
`VELA_WITHDRAWAL_RPC_PRIMARY_*` and `VELA_WITHDRAWAL_RPC_SECONDARY_*`. URL
userinfo is rejected. Request timeouts, retry ceilings, polling, lease duration,
and batch size are positive and bounded through the corresponding
`VELA_WITHDRAWAL_*` settings. Never put an RPC credential in logs or command
output; the worker's provider errors identify the configured provider name but
do not render URLs or response bodies.

At startup both providers must agree on chain ID and the exact finalized block.
Both must return runtime bytecode whose keccak256 matches the immutable enabled
V3 deployment registry row. The registered execution topic must match the V3
ABI. A PostgreSQL deployment lease prevents concurrent owners, while a
monotonic scan-cycle checkpoint makes crash replay safe. The lease must cover
the worst-case bounded RPC sequence and is renewed before and after each
candidate. Ctrl-C releases an owned lease.

Each cycle repeatedly selects actionable withdrawals by `updated_at, id` and
always observes only the latest immutable broadcast attempt. Provider agreement
is required on the finalized block, exact transaction fields, receipt block,
and the complete `WithdrawalExecuted` event, including deadline, policy hash,
and approver hash. Matching execution evidence calls the existing atomic ledger
completion method.

Two-provider `not found` evidence may mark only the current attempt dropped. It
never releases funds. EX-028 records a finalized revert as immutable terminal
evidence for the exact attempt, while retaining the reservation for as long as
the signed authorization could still execute. Release is considered only when the agreed finalized
block timestamp is after the authorization deadline and both providers prove
the request ID and customer nonce unused, no matching event from the registered
deployment start block, and the database proves every attempt terminal. A
reverted receipt remains ambiguous under the still-valid authorization and is
not released early. The database independently checks the recorded attempt
lineage is terminal before accepting the final release, rather than trusting the
worker's boolean alone.

This remains testnet-only. Before running it, externally review the HTTP RPC
adapter, contract deployment, registry evidence, provider independence, RPC log
range limits, alerting, and operational recovery. Do not use real funds.
