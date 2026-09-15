# Vela Consumer Exchange Engineering Plan

**Status:** Active planning document  
**Initial scope:** Spot ETH/USDC only  
**Current operating state:** Private repository, testnet only, no real funds  
**Master backlog:** [Consumer exchange launch checklist](consumer-exchange-launch-checklist.md)

## Objective

Build a narrowly scoped Vela release in which every customer balance is backed, every trade is reflected in the authoritative financial ledger, every withdrawal is tied atomically to that ledger, and the system fails closed when correctness cannot be established.

Matching speed is not the critical path. Financial correctness, custody, recovery, and honest product behavior are the critical path.

## Accepted constraints

- Vela remains testnet-only until every launch gate in this plan is satisfied.
- The first real-money product is spot ETH/USDC only.
- No perps, margin, borrow and lend, portfolio margin, public LP vaults, MM credit, permissionless listings, or cross-chain deposits are included in the first release.
- Work on AI-agent features, additional exotic order types, and nonessential FIX expansion is paused.
- No U.S. real-money launch proceeds until counsel defines the allowed entity, customer, asset, and jurisdiction model.
- No mainnet settlement contract is deployed before the redesigned custody and withdrawal system is externally audited.
- Synthetic liquidity and balances are never permitted in production.
- Public claims must describe implemented guarantees, not roadmap intent.

## Provisional system architecture

This plan uses a two-stage hybrid architecture unless an ADR explicitly replaces it.

### Stage 1: ledger-first exchange

- An on-chain custody vault holds customer assets.
- A durable double-entry ledger is the authoritative record of customer ownership.
- The matching engine is an execution service and derived cache, not the source of financial truth.
- A chain indexer observes finalized deposit events and proposes ledger credits.
- A withdrawal orchestrator reserves ledger funds before requesting a custody signature or broadcast.
- A reconciliation service compares custody assets with ledger liabilities continuously.
- The platform halts affected financial operations when reconciliation or required infrastructure is unhealthy.

### Stage 2: progressive trust minimization

- Commit post-trade balance roots on-chain.
- Make finalized balance data independently reproducible.
- Add proof-based withdrawal rights.
- Add forced inclusion and a real delayed inbox.
- Add enforceable fraud proofs or validity proofs.
- Make emergency exits derive from finalized post-trade ownership.

Stage 2 is not allowed to weaken or replace the accounting guarantees delivered by Stage 1.

## Core invariants

The implementation and tests must preserve these invariants at all times:

1. Every finalized customer credit has independently verifiable custody evidence.
2. Every journal transaction balances to zero across debit and credit legs.
3. Total customer liabilities for an asset never exceed reconciled custody for that asset.
4. A deposit event can affect the ledger at most once.
5. A withdrawal can leave custody only after the matching ledger amount is reserved or debited.
6. Failed withdrawals restore the reserved amount exactly once.
7. A trade transfers ownership atomically between buyer and seller.
8. A seller cannot withdraw or emergency-exit an asset after selling it.
9. A buyer can withdraw an asset after acquiring it and reaching finality.
10. Production cannot start with test fixtures, synthetic balances, temporary financial storage, testnet contracts, or placeholder security providers.
11. Recovery produces the same balances, orders, fills, fees, and withdrawal states as the pre-failure journal.
12. An unhealthy ledger, indexer, signer, custody connection, or reconciler prevents unsafe operations.

## Milestone 0: containment and operating modes

**Goal:** Make it impossible to mistake or configure the current demo as a production exchange.

**Target duration:** 2 to 4 engineering days.

### M0 tasks

- [x] Introduce one typed runtime-mode abstraction with `development`, `testnet-demo`, and `production` values.
- [x] Route all environment checks through the typed runtime mode.
- [x] Preserve compatibility with the existing `ENVIRONMENT=production` guard during migration.
- [x] Add a startup validation report that lists only nonsecret configuration and enabled capabilities.
- [x] Make production reject Sepolia chain ID.
- [x] Make production reject known testnet settlement addresses.
- [x] Make production reject synthetic balance seeding.
- [x] Make production reject synthetic order-book seeding.
- [x] Make production reject the internal unfunded MM bot.
- [x] Make production reject temporary or fallback WAL storage.
- [x] Make production reject demo RPC URLs.
- [x] Make production reject placeholder ZK proofs when ZK is advertised as active.
- [x] Make production reject simulated TEE attestations when TEE is advertised as active.
- [x] Restrict the current direct-credit `POST /deposit` behavior to `testnet-demo`.
- [x] Rename or label demo credits so they cannot be confused with verified chain deposits.
- [x] Disable production withdrawal signing until ledger reservation exists.
- [x] Remove any frontend use of `NEXT_PUBLIC_ADMIN_TOKEN`.
- [x] Add a public sanitized reserves endpoint that does not require an exposed administrative credential.
- [x] Correct homepage, README, and in-app claims about custody, settlement, fraud proofs, forced inclusion, and withdrawals.
- [x] Keep the testnet and no-real-funds banner prominent.
- [x] Diagnose and recover the deployed Fly engine health and status timeouts. Root-owned persistence files, an incompatible snapshot, and a missing `ADMIN_TOKEN` caused the crash loop; the old snapshot was preserved and the repaired service reports operational.

### M0 acceptance criteria

- [x] Unit tests prove every unsafe configuration causes production startup to fail.
- [x] Production cannot create or credit a synthetic account.
- [x] Production cannot call the current unverified deposit-credit path.
- [x] Production cannot issue a withdrawal signature through the current unreserved flow.
- [x] No private value is bundled into the frontend through a `NEXT_PUBLIC_*` variable.
- [x] Public documentation consistently describes the current testnet trust model.
- [x] Existing Rust, Solidity, and frontend checks continue to pass.

## Milestone 1: architecture decisions and financial model

**Goal:** Resolve the financial design before implementing infrastructure.

**Target duration:** 3 to 5 engineering days.

### Required ADRs

- [x] ADR-001: Authoritative ledger and consistency model.
- [x] ADR-002: Custody contract model, including omnibus versus per-user accounting.
- [x] ADR-003: Deposit state machine, finality, reorganization, and idempotency rules.
- [x] ADR-004: Withdrawal state machine, reservation, signing, broadcast, and reversal rules.
- [x] ADR-005: Matching-engine integration and atomic journal boundary.
- [x] ADR-006: Reconciliation, halt policy, and administrative correction model.
- [x] ADR-007: Recovery, replication, fencing, RPO, and RTO.
- [x] ADR-008: Initial chain selection and ETH/USDC asset representations.

### Required data model

- [x] Define accounts for customer available, customer locked, customer pending, custody, fees, rebates, and operational adjustments.
- [x] Define immutable journal transactions and journal legs.
- [x] Define external references and idempotency keys.
- [x] Define deposits and their observed chain events.
- [x] Define withdrawals and their full state history.
- [x] Define custody snapshots and reconciliation results.
- [x] Define administrative corrections with reason, evidence, actor, and approval records.
- [x] Define schema versioning and migration rules.

### M1 acceptance criteria

- [x] Each core invariant maps to a documented owner and test strategy.
- [x] Deposit and withdrawal state transitions are explicit and exhaustive.
- [x] Failure behavior is documented for every external dependency in the accepted architecture.
- [x] The matching engine can be rebuilt from the planned financial journal.
- [x] No unresolved architecture choice is hidden inside an implementation ticket.

## Milestone 2: authoritative double-entry ledger

**Goal:** Establish a durable financial source of truth.

**Target duration:** 1 to 2 engineering weeks.

### M2 tasks

- [ ] Provision the selected managed durable database; PostgreSQL 16 schema, SQLx migrations, and CI test service are implemented.
- [x] Implement append-only journal transactions and balanced journal legs.
- [x] Reject unbalanced journal transactions at the database boundary.
- [x] Add globally unique idempotency keys.
- [x] Add account-level sequence or version controls for concurrency.
- [x] Implement atomic balance materialization from journal entries.
- [x] Implement available, locked, and pending balance categories.
- [x] Journal matcher-produced fees and maker rebates explicitly.
- [x] Add auditable administrative adjustment transactions.
- [x] Require dual approval for production administrative adjustments.
- [x] Build deterministic financial-balance replay from the journal.
- [x] Integrate the matcher so accepted trades produce atomic ownership transfers.
- [x] Prevent the matcher from acknowledging a financially effective result before the ledger commits.
- [x] Remove the engine's role as authoritative owner of customer funds; customer balance reads use the ledger projection, the engine is a replayed execution cache, and unjournaled mutation routes fail closed in ledger-backed mode.

### M2 acceptance criteria

- [x] Property tests prove every journal transaction balances.
- [x] Concurrent operations cannot overspend an account.
- [x] Duplicate requests return the original result without applying twice.
- [x] A full ledger replay reproduces all materialized account balances exactly.
- [x] A completed trade moves base, quote, fees, and rebates atomically.
- [x] Engine restart reloads customer available/locked ownership and platform fees from the authoritative journal.

## Milestone 3: verified deposit pipeline

**Goal:** Credit only finalized, independently verified custody events.

**Target duration:** 1 to 2 engineering weeks.

### M3 tasks

- [x] Build a dedicated chain indexer for the selected launch chain; the provider-agnostic Base service is implemented and locally verified, while managed hosting remains open.
- [x] Observe the correct contract and deposit event signature; the source now emits and the indexer strictly decodes `Deposited(bytes32,address,address,uint256)`. The legacy Sepolia deployment is intentionally incompatible and must not be registered.
- [x] Record chain ID, contract, block number, block hash, transaction hash, transaction index, and log index in the durable observation command.
- [x] Verify depositor, beneficiary, asset, and actual received amount; the current self-deposit ABI binds depositor and beneficiary to the indexed user, allowlists the ledger asset, and emits the measured ERC20 balance delta.
- [x] Complete observed, confirming, finalized, credited, rejected, orphaned, quarantined, and reversed workflows; credited contradictions atomically halt the asset and either isolate the full amount into pending or quarantine an exact shortfall.
- [x] Define confirmation thresholds per asset and chain; launch-scope Base ETH and USDC both use the pinned `rpc_finalized_two_provider` policy, and latest-head confirmation counts are evidence only.
- [x] Detect and process chain reorganizations; a cyclic two-provider audit rechecks every already-processed finalized range and never rewinds its checkpoint except through a guarded cycle reset to the immutable deployment block.
- [x] Make canonical event identity unique and observation idempotent.
- [x] Journal final deposit credits through the ledger atomically with credited state and immutable history.
- [x] Add redundant RPC providers with distinct configured names and URL origins.
- [x] Detect stale providers and provider disagreement before observation, credit, or checkpoint advancement.
- [x] Expose deposit progress and evidence to the customer API; the ledger-backed address route returns exact amounts, all guarded states, and canonical chain-event identity, with optional real transaction-hash filtering.
- [x] Update the frontend to submit or discover the real transaction hash; configured Base wallet submissions are tracked through the authoritative status route and rendered with their block-explorer identity.
- [x] Remove the invented transaction-hash path from the verified frontend flow; a confirmed wallet transaction never calls the separately contained demo-credit endpoint.

### M3 acceptance criteria

- [ ] No API request can credit funds without a verified chain event.
- [x] The same canonical log cannot be recorded or credited twice at the ledger boundary.
- [x] Reorganizations do not leave invalid finalized credits usable; the database blocks affected-asset credits, trading, and withdrawals before complete reversal or shortfall quarantine.
- [x] Provider failure cannot silently produce a credit at the indexer boundary; timeout, stale head, chain mismatch, bytecode mismatch, log disagreement, and finalized disagreement all fail closed.
- [x] Customers can see observed, confirming, finalized, credited, rejected, orphaned, reversed, and quarantined deposit status from the authoritative ledger.
- [ ] Custody and ledger amounts reconcile after every finalized deposit.

## Milestone 4: atomic withdrawal pipeline

**Goal:** Make every asset movement out of custody correspond exactly to ledger ownership.

**Target duration:** 1 to 2 engineering weeks.

### M4 tasks

- [x] Authenticate the customer withdrawal request; the wallet signature binds request ID, customer, destination, asset, exact amount, and nonce.
- [x] Validate available balance and locked exposure at the authoritative account boundary; only `customer_available` can fund a reservation.
- [x] Atomically reserve the ledger amount in `customer_withdrawal_reserved`.
- [x] Persist the withdrawal before requesting a custody signature; the legacy signer fails closed whenever the authoritative ledger is configured.
- [x] Bind signing authorization to chain, vault contract, user, destination, asset code and token address, amount, nonce, expiry, policy, approver, and request ID in an immutable Keccak-256 envelope.
- [x] Replace the authoritative withdrawal path's environment-held signing key with a provider-neutral HSM or MPC adapter boundary that is disabled by default; its deterministic signer is test-only.
- [ ] Track requested, reserved, approved, signed, submitted, confirmed, failed, canceled, and reversed states; the source-only V2 boundary now persists broadcast, replacement, finalized confirmation, and finalized expiry failure, but no runtime worker drives the complete lifecycle.
- [ ] Observe transaction replacement and dropped transactions; the opt-in worker continuously verifies current attempts, records exact finalized reverts, and verifies supplied replacement hashes against two providers, but a controlled relay, trusted hash feed, or separately operated scanner must still supply replacement candidates.
- [x] Restore reservations exactly once after permanent failure, and only after finalized two-provider evidence proves the V2 authorization expired with its request ID and nonce unused, no execution event, and all attempts terminal.
- [ ] Implement ETH and USDC withdrawals through the same state machine; the V2 contract source supports native and ERC20 transfers, but deployed end-to-end integration is not complete.
- [x] Add per-user, per-asset, and platform withdrawal limits in atomic asset units, with customer count and amount velocity windows.
- [x] Add a durable global withdrawal halt that blocks new reservation, approval, and signing, with three-party resolution evidence, while permitting safe release and mandatory reconciliation of already-finalized execution.
- [x] Add risk and compliance policy hooks without hardcoding a specific vendor; unavailable or review dispositions fail closed and no external screening provider is configured yet.
- [x] Expose actionable withdrawal reservation status to the frontend, including a signed pre-signing cancellation that releases funds exactly once.

### M4 acceptance criteria

- [ ] Custody cannot release funds without a matching durable reservation.
- [ ] A failed withdrawal cannot permanently destroy customer funds.
- [x] Replayed V2 requests and signatures cannot withdraw twice at the contract and ledger evidence boundaries; no deployment or real-funds claim is implied.
- [x] Concurrent withdrawals cannot exceed available balance; serializable reservation tests prove exactly one competing debit succeeds.
- [ ] Confirmed withdrawals reconcile across custody and ledger.
- [ ] ETH and USDC pass identical end-to-end state-machine tests.

## Milestone 5: custody and settlement contract redesign

**Goal:** Make on-chain custody compatible with post-trade ownership.

**Target duration:** 2 to 4 engineering weeks before external audit.

### M5 tasks

- [ ] Replace the current disconnected per-user deposit ledger.
- [ ] Implement the custody model selected in ADR-002.
- [ ] Add an explicit supported-asset allowlist.
- [ ] Account for actual token amounts received.
- [ ] Reject unsupported and fee-on-transfer assets.
- [ ] Define contract pause and withdrawal-halt behavior.
- [ ] Define owner, operator, guardian, and signer powers.
- [ ] Define safe role rotation and recovery.
- [ ] Implement replay-safe withdrawal authorization.
- [ ] Define a safe migration path if contracts remain immutable.
- [ ] Add deposit, trade, withdrawal, pause, migration, and recovery invariants.
- [ ] Add a mainnet-fork integration harness.
- [ ] Update deployment scripts and make them reproducible.
- [ ] Remove unused contract deployments from the initial runbook.

### M5 acceptance criteria

- [ ] A seller cannot reclaim an asset after selling it.
- [ ] A buyer can withdraw a finalized acquired asset.
- [ ] Contract assets cover reconciled ledger liabilities.
- [ ] All fund-moving paths are tested under pause, failure, and replay conditions.
- [ ] Deployment artifacts identify chain, bytecode, configuration, and source commit.
- [ ] The complete contract and off-chain settlement surface is ready for external audit.

## Milestone 6: reconciliation, recovery, and high availability

**Goal:** Detect insolvency immediately and recover deterministically from infrastructure failure.

**Target duration:** 1 to 2 engineering weeks.

### M6 tasks

- [x] Build continuous per-asset custody-to-liability reconciliation; the testnet worker remains separately opt-in and unconfigured.
- [x] Persist immutable reconciliation snapshots with exact finalized block, dual-provider, deployment, bytecode, custody, and ledger-projection evidence.
- [x] Automatically halt deposit credits, trading, and new withdrawals for deficits or indeterminate evidence; services never automatically unhalt.
- [x] Add independent reconciliation execution outside the matcher process as a leased, restart-safe worker.
- [x] Remove temporary WAL fallback behavior in every runtime mode; startup uses only the configured durable directory.
- [ ] Fail closed on corrupt or unavailable financial storage.
- [ ] Replicate ledger and journal storage across failure domains.
- [ ] Create encrypted offsite backups.
- [ ] Implement point-in-time recovery.
- [ ] Add leader fencing for engine failover.
- [ ] Test recovery while deposits, orders, trades, and withdrawals are in flight.
- [ ] Define and measure RPO and RTO.
- [x] Add ledger connectivity and schema-version readiness checks.
- [ ] Add readiness checks for indexer, signer, custody RPC, and reconciler.
- [ ] Add paging for failed writes, divergence, stuck withdrawals, and unhealthy dependencies.

### M6 acceptance criteria

- [ ] Any liability or custody mismatch generates an alert and safe halt.
- [ ] Recovery from the journal produces identical balances and financial states.
- [ ] Two engine leaders cannot process financially effective orders simultaneously.
- [ ] Backup restoration and point-in-time recovery pass a documented rehearsal.
- [ ] The API cannot report ready while a required financial dependency is unhealthy.

## Milestone 7: constrained consumer product

**Goal:** Deliver a usable ETH/USDC spot experience without weakening security boundaries.

**Target duration:** 1 to 2 engineering weeks.

### M7 tasks

- [ ] Implement authenticated HTTP access to balances, orders, fills, portfolio data, and exports.
- [ ] Complete session-key creation and revocation.
- [ ] Support scoped, expiring session permissions.
- [x] Complete ETH and USDC deposit status interfaces; both assets share the fail-closed Base configuration and ledger-backed activity view, while submission remains disabled until the compatible vault is deployed and configured.
- [ ] Complete ETH and USDC withdrawal status interfaces.
- [ ] Add transaction receipts and downloadable history.
- [ ] Add fee previews and clear rejection messages.
- [ ] Add deposit, withdrawal, account, and platform limits to the UI.
- [ ] Add disconnected, degraded, pending, empty, and failure states.
- [ ] Add mobile-wallet and WalletConnect support.
- [ ] Add real Terms, Privacy, risk, custody, fee, and jurisdiction pages after counsel review.
- [ ] Add customer-support and complaint-reporting paths.
- [ ] Expand frontend unit, integration, and end-to-end coverage.

### M7 acceptance criteria

- [ ] A wallet can access only its own private records.
- [ ] A consumer can complete deposit, trade, and withdrawal without an unexplained manual step.
- [ ] Every financial state shown in the UI comes from an authoritative backend state.
- [ ] Mobile and supported wallet flows pass automated and manual verification.
- [ ] Product language matches audited implementation and legal review.

## Milestone 8: hardening and launch qualification

**Goal:** Produce evidence for a real-money go or no-go decision.

**Target duration:** Driven by external audit, legal, and operational readiness.

### M8 tasks

- [ ] Commission external smart-contract and settlement-system audits.
- [ ] Commission an API and infrastructure penetration test.
- [ ] Fix all Critical and High findings.
- [ ] Make dependency and secret scanning blocking.
- [ ] Launch a funded bug bounty.
- [ ] Run a capped adversarial testnet.
- [ ] Run sustained load, reconnect-storm, failover, and chaos tests.
- [ ] Rehearse contract pause, signer compromise, RPC failure, database loss, reorganization, and insolvency response.
- [ ] Establish on-call ownership and escalation.
- [ ] Establish a status page and customer notification process.
- [ ] Obtain written legal approval for supported customers, assets, and jurisdictions.
- [ ] Secure real funded market makers.
- [ ] Add market-surveillance, price collars, circuit breakers, and operational kill switches.
- [ ] Freeze and sign the audited release artifacts.
- [ ] Complete a mainnet-fork and deploy-day rehearsal.

### M8 acceptance criteria

- [ ] Custody launch gate is green.
- [ ] Security launch gate is green.
- [ ] Legal launch gate is green.
- [ ] Operations launch gate is green.
- [ ] Market-quality launch gate is green.
- [ ] A written go or no-go review approves strict initial customer and value caps.

## Launch gates

### Custody gate

- Every credited balance has custody evidence.
- Every completed trade transfers ledger ownership atomically.
- Every withdrawal is reserved, tracked, confirmed, and reconciled.
- Emergency and migration behavior preserve current customer ownership.

### Security gate

- The complete financial path has been externally audited.
- All Critical and High findings are resolved and verified.
- Keys use approved hardware or MPC custody.
- Production access and deployments use least privilege and approval controls.

### Legal gate

- The operating entity and product classification are documented.
- Supported jurisdictions and excluded jurisdictions are enforceable.
- Customer onboarding and compliance procedures are operational.
- ETH and USDC support has received asset-specific review.

### Operations gate

- Reconciliation, monitoring, backups, recovery, and failover are tested.
- Incident response and customer communications are rehearsed.
- On-call ownership exists for every production dependency.
- The system fails closed when financial correctness is uncertain.

### Market-quality gate

- Real funded market makers are contracted and monitored.
- Reference pricing uses redundant sources.
- Market surveillance and manipulation controls are operational.
- Price collars, circuit breakers, mass cancel, and exchange halt controls are tested.

## First implementation queue

Work should begin in this order. Each item should land as a small, independently reviewable change.

1. [x] **EX-001: Typed runtime mode**
   - Centralize runtime-mode parsing and capability checks.
   - Add a nonsecret startup configuration report.
2. [x] **EX-002: Production startup validator**
   - Reject testnet, synthetic, fallback-storage, and placeholder configurations.
3. [x] **EX-003: Synthetic state isolation**
   - Allow seed balances, books, and the internal MM only in `testnet-demo`.
4. [x] **EX-004: Direct deposit containment**
   - Restrict unverified credits to `testnet-demo` and label them as demo credits.
5. [x] **EX-005: Withdrawal-signing containment**
   - Refuse production signing until a durable reservation adapter exists.
6. [x] **EX-006: Frontend secret boundary**
   - Remove `NEXT_PUBLIC_ADMIN_TOKEN` and replace the reserves data flow.
7. [x] **EX-007: Trust-model copy correction**
   - Align README, homepage, docs, and product banners with actual guarantees.
8. [x] **EX-008: Production-guard test matrix**
   - Prove each prohibited production configuration fails closed.
9. [x] **EX-009: Fly health investigation**
   - Determine why health and status requests time out and document the resolution.
   - Root cause, recovery actions, and verification evidence: [Fly engine health runbook](operations/fly-health.md).
10. [x] **EX-010: ADR set**
    - Complete ADR-001 through ADR-008 before starting the financial ledger implementation.
11. [x] **EX-011: Ledger data model and migrations**
    - Add the PostgreSQL schema for accounts, journal records, deposits, withdrawals, custody snapshots, reconciliation, administrative corrections, and the transactional outbox.
    - Document migration rules, invariant ownership, and test strategy.
12. [x] **EX-012: Atomic ledger posting primitive**
    - Commit balanced journal legs, balance projections, versions, and outbox records in one serializable transaction.
    - Prove database balance enforcement, immutability, idempotent retry behavior, projection consistency, and concurrent overspend prevention against PostgreSQL.
13. [x] **EX-013: PostgreSQL runtime safety and controlled migrations**
    - Add bounded pooling, statement timeouts, serializable retries, exact schema-version readiness, and production startup guards.
    - Separate writer and migrator credentials, add least-privilege database roles, and provide a manually approved migration workflow.
    - Document the backup policy and verify a logical dump/restore locally against PostgreSQL 16.
14. [ ] **EX-014: Managed PostgreSQL provisioning and disaster recovery**
    - Select and provision isolated managed production and staging PostgreSQL 16 services when budget permits.
    - Configure replication, encrypted offsite backups, point-in-time recovery, alerts, and provider-specific restore rehearsal before accepting real funds.
15. [x] **EX-015: Matcher-ledger financial commit boundary**
    - Execute order batches against private proposal state, derive balanced customer and platform account effects, and publish engine state only after the authoritative ledger commits.
    - Journal order reservations, releases, trades, taker fees, maker fees, maker rebates, and referral payouts with deterministic idempotency keys and exact asset-decimal conversion.
    - Fail closed without acknowledging or publishing speculative state when validation, account resolution, or PostgreSQL commit fails; expose a rejection counter for alerting.
    - Verify acknowledgement ordering, rollback behavior, and a complete ETH/USDC trade against PostgreSQL 16.
16. [x] **EX-016: Ledger account provisioning and financial replay**
    - Add idempotent, policy-checked account provisioning for approved assets, customer available/locked balances, and platform fee/rebate accounts.
    - Add an explicit ETH/USDC bootstrap command that verifies chain and asset configuration and is run only with the migrator credential.
    - Replay journal legs in sequence under a consistent PostgreSQL snapshot, verify every replayed value against the materialized projection, and fail startup on divergence.
    - Replace snapshot/WAL financial balances with replayed customer ownership and platform fee balances before ledger-backed API traffic starts.
    - Verify restart ownership, stale-cache replacement, decimal exactness, account idempotency, configuration conflict rejection, and projection-corruption detection against PostgreSQL 16.
17. [x] **EX-017: Dual-control administrative corrections and ledger properties**
    - Add a typed correction command with a fixed reason taxonomy, evidence reference, incident ID, initiator, independent approver, approval time, and balanced legs.
    - Bind the complete correction request into a deterministic hash and commit its journal, projection, outbox, and immutable audit record atomically.
    - Reject self-approval, unaudited generic administrative postings, conflicting retries, and incomplete audit writes before any financial effect can persist.
    - Generate and post randomized balanced multi-leg transactions, then verify every persisted transaction remains balanced per asset and every projection matches journal history.
18. [x] **EX-018: Ledger-authoritative customer balance boundary**
    - Serve customer REST balances and authenticated WebSocket balance snapshots from PostgreSQL's transactionally maintained ledger projection whenever the ledger is configured.
    - Fail closed on projection read or exact-decimal conversion errors instead of falling back to local matching-engine state.
    - Reject every legacy API path that directly changes the engine balance map while the ledger is authoritative, including demo/bridge credits, withdrawals, forced inclusion, RFQ settlement, subaccount transfers, vault flows, and listing bonds.
    - Preserve those synthetic flows only for explicitly non-authoritative development and `testnet-demo`, and prove an external credit cannot mutate a ledger-backed engine projection.
19. [x] **EX-019: Durable verified-deposit ledger foundation**
    - Add typed observation, state, two-provider finality, orphaning, and finalized-credit commands over the existing deposit evidence tables.
    - Enforce canonical case-insensitive chain-event identity, immutable deposit facts, valid state transitions, and same-transaction history in PostgreSQL migration 0002.
    - Atomically post the custody/customer journal movement, balance projections, outbox event, credited state, and history under a chain-derived idempotency key.
    - Provision per-asset custody-vault counter-accounts and verify exact retries, conflicting evidence, provider disagreement, pre-finality rejection, reorg orphaning, transition bypass, atomic credit, and double-credit prevention against PostgreSQL 16.
20. [x] **EX-020: Fail-closed Base deposit indexer and vault event boundary**
    - Replace the ambiguous three-field vault event with a deployment-scoped indexed deposit ID and emit the measured ERC20 balance delta as the authoritative amount.
    - Add an immutable Base deployment registry with pinned runtime bytecode hash, event ABI/topic, deployment block, and two-provider finalized policy.
    - Add a dedicated indexer binary with distinct RPC-origin validation, request timeouts, stale-head and lag detection, highest-common-finalized hash agreement, bytecode verification, exact normalized log agreement, and strict ABI decoding.
    - Route verified ETH/USDC events through deterministic observation, confirming, finality, and atomic ledger credit commands; reject unregistered assets and missing custody accounts.
    - Add a PostgreSQL single-worker lease, monotonic restart checkpoint, immutable halt evidence, bounded retry/backoff, graceful shutdown, controlled deployment-registration command, and migration-workflow grants.
    - Verify exact credit and checkpointing, restart idempotency, malformed and duplicate log rejection, provider disagreement, stale heads, wrong bytecode, lease exclusion, registry immutability, and checkpoint rewind prevention against PostgreSQL 16.
21. [x] **EX-021: Credited-deposit contradiction containment and reorganization recovery**
    - Add a separately guarded cyclic checkpoint that continuously re-audits previously processed finalized ranges against identical normalized logs from both RPC providers.
    - Detect missing, changed, or newly appeared historical deposit events behind the monotonic forward checkpoint, and require provider agreement before any financial recovery command.
    - Atomically activate an append-only asset-wide halt before moving a contradicted credit from customer available into customer pending; mark complete isolation `reversed` and any spent-funds shortfall `quarantined` with exact evidence.
    - Enforce halted-asset deposit-credit, trading, fee, rebate, and new withdrawal-reservation rejection at the PostgreSQL journal boundary while preserving safe releases, reversal, and mandatory journaling of any already-finalized custody execution.
    - Require evidence plus distinct initiator and approver identities to resolve a halt, and withhold resolution permission from the restricted runtime writer.
    - Verify complete reversal, partial-balance isolation, exact shortfall quarantine, idempotent retry, blocked financial operations, immutable halt evidence, audit-provider disagreement, and audit-checkpoint rewind prevention against PostgreSQL 16.
22. [x] **EX-022: Authoritative customer deposit status and real transaction discovery**
    - Add a bounded ledger query scoped case-insensitively to one customer reference, with optional canonical transaction-hash filtering and exact asset-decimal rendering.
    - Expose every guarded deposit state plus canonical chain evidence through `GET /account/{address}/deposits`, and fail with `503` when the authoritative ledger is absent or unavailable.
    - Remove the on-chain ETH/USDC frontend handoff to `POST /deposit`; wallet submission and confirmation create no balance until the indexer and ledger credit the canonical event.
    - Add explicit Base and Base Sepolia public configuration, reject every other chain, and keep verified submission disabled until a valid vault and asset address are supplied.
    - Poll and display ledger-backed deposit activity with the real transaction hash while preserving the separately labeled non-authoritative demo flow for other testnet assets.
    - Verify customer scoping, transaction filtering, exact decimal amounts, fail-closed ledger absence, frontend configuration guards, generated API types, and the existing Rust and frontend suites.
23. [x] **EX-023: Atomic withdrawal reservation and customer status**
    - Replace authoritative-mode matching-engine withdrawal mutation with a serializable PostgreSQL reservation from customer available to customer withdrawal-reserved ownership.
    - Bind the customer's EIP-191 authorization to a UUID request ID, normalized customer and destination addresses, ETH/USDC asset, exact amount, and nonce.
    - Make identical retries return the original journal transaction, reject conflicting ID or nonce reuse, and prove concurrent reservations cannot overspend.
    - Add guarded database transitions, immutable withdrawal identity, mandatory same-transaction state history, and an exactly-once pre-signing cancellation release.
    - Fail closed on the legacy environment-held custody signer whenever the ledger is authoritative; custody approval, signing, broadcast, completion, and reconciliation remain later M4/M5 work.
    - Expose ledger-only customer status and update the frontend to preflight, reserve, poll, explain, and safely cancel ETH/USDC withdrawal requests.
24. [x] **EX-024: Contract-bound custody signer adapter**
    - Persist a versioned authorization envelope binding request ID, chain, vault contract, customer, destination, asset code and token address, atomic amount, nonce, expiry, policy version, and approver.
    - Hash the canonical envelope with Keccak-256, expose only its immutable digest to a provider-neutral asynchronous `CustodySigner` interface, and recover the returned signature against its configured secp256k1 public key.
    - Guard `reserved` to `approved` to `signing` to `signed` transitions in PostgreSQL and require matching append-only authorization and recoverable-signature evidence.
    - Make authorization and signature retries return the original evidence, reject conflicting identifiers or digests, and preserve the financial reservation throughout signing.
    - Keep the runtime signer disabled and place the deterministic secp256k1 implementation behind tests only, with no vendor account, deployable key, broadcast, or real-funds path.
    - Verify exact field binding, signature recovery, evidence immutability, transition bypass rejection, and signed-state reservation preservation.
25. [x] **EX-025: Withdrawal risk, velocity, and halt policy boundary**
    - Add immutable, versioned per-asset policy rows with exact atomic single-withdrawal caps, customer amount/count windows, and platform amount windows.
    - Evaluate each reserved withdrawal in a serializable transaction, including the current reservation in velocity totals, and persist the limits, observations, provider disposition, reason codes, and SHA-256 evidence hash.
    - Require the active policy version, an approved decision, and the same evaluator identity before PostgreSQL accepts a contract-bound custody authorization.
    - Add a provider-neutral compliance disposition contract in which `reject` is denied and `review` or `unavailable` requires review; no external sanctions or blockchain-analytics provider is implied or configured.
    - Add a global kill switch that blocks new withdrawal reservations, approval, signing, and signature storage, while preserving safe releases and mandatory journaling of any already-finalized custody execution.
    - Make halt activation replay-safe and require immutable evidence plus three distinct operator identities across activation, resolution initiation, and final resolution.
    - Verify per-withdrawal, customer amount/count, platform velocity, compliance-unavailable, authorization-bypass, replay, and global halt behavior against PostgreSQL 16.
26. [x] **EX-026: V2 relay, execution finality, and permanent-failure accounting boundary**
    - Replace the legacy caller-bound contract authorization in source with a permissionlessly relayed EIP-712 V2 payload binding request ID, customer, destination, asset, amount, nonce, deadline, policy hash, approver hash, chain, and deployment.
    - Consume request IDs and per-customer nonces before transferring ETH or ERC20, emit the complete signed execution identity, and verify relay, replay, expiry, domain separation, evidence binding, and a fixed cross-language digest vector in Foundry.
    - Register immutable V2 contract capabilities and versioned two-provider finalized confirmation policies before PostgreSQL accepts an executable authorization.
    - Persist immutable broadcast attempts and immediate-predecessor replacement lineage, while keeping the provider-neutral chain observer disabled by default and its deterministic provider test-only.
    - Atomically store finalized matching execution evidence, journal `customer_withdrawal_reserved` to `custody_withdrawn`, and transition to `confirmed` exactly once.
    - Release an expired reservation exactly once only after finalized proof that the request ID and nonce remain unused, no execution event exists, and every attempt is terminal; ambiguous transactions remain reserved.
    - Keep the entire chapter source- and test-only: no contract deployment, runtime broadcaster or observer worker, real RPC provider, production signer, or funds are enabled.
27. [x] **EX-027: Restart-safe withdrawal observation worker**
    - Add a separate, explicitly enabled Rust worker that monitors previously recorded withdrawal attempts without holding a relayer or custody key; production broadcasting remains disabled.
    - Require two distinct HTTPS RPC origins and provider identities, bounded request timeouts and retry backoff, matching chain ID and finalized checkpoint, pinned runtime bytecode, and the registered V2 event topic before processing.
    - Verify the current transaction, canonical receipt, and complete `WithdrawalExecuted` event, including request, customer, destination, asset, amount, nonce, deadline, policy hash, and approver hash, before invoking exactly-once ledger completion.
    - Persist a deployment-scoped single-owner lease and durable `(updated_at, withdrawal_id)` round-robin cursor so crashes replay safely and indefinitely pending rows cannot starve later work.
    - Mark only the current attempt dropped from matching two-provider `not found` evidence; release only after an agreed finalized post-deadline block proves the request and nonce unused, no matching event since deployment, and every attempt terminal.
    - Fail closed on provider disagreement, malformed evidence, insufficient depth, or finalized reverts; reverted transactions remain reserved until a future exact revert-evidence migration or reviewed replacement path exists.
    - Keep the worker unconfigured and stopped by default. No RPC credentials, deployment changes, private keys, transaction broadcast, or funds are introduced by this chapter.
28. [x] **EX-028: Finalized-revert evidence and verified replacement lineage**
    - Persist exact finalized status-zero receipts as immutable, one-to-one attempt evidence after two independent providers agree on the transaction, canonical inclusion block, finalized checkpoint, and confirmation policy.
    - Treat a finalized revert as terminal for that attempt without releasing its reservation while the signed authorization remains executable; post-expiry release still requires finalized unused request and nonce, no-event, and database-derived all-attempts-terminal proof.
    - Add a typed replacement-hash ingestion boundary that verifies a supplied candidate with both providers against the latest attempt's sender, Ethereum account nonce, vault, and authorization calldata hash before atomically recording its immutable predecessor lineage.
    - Require every attempt after the first to carry immediate-predecessor and two-provider replacement evidence, enforce the invariant with a deferred database constraint, and fail migration if unverifiable legacy replacement rows already exist.
    - Keep the capability honest: standard JSON-RPC cannot enumerate arbitrary pending same-nonce replacements from the predecessor hash, so automatic mempool discovery remains open until a reviewed relay, trusted hash feed, or separately operated scanner supplies candidates.
    - Keep the worker opt-in and the broadcaster disabled. No RPC credentials, deployment, signing keys, transaction submission, or funds are introduced by this chapter.
29. [x] **EX-029: Continuous custody-to-liability reconciliation**
    - Add a separate, explicitly enabled reconciliation worker with bounded RPC timeouts, retry backoff, responsive shutdown, and a deployment-scoped PostgreSQL lease.
    - Require two distinct RPC origins to agree on chain ID and the exact finalized block, then verify the pinned vault runtime bytecode and read ETH and registered ERC20 custody balances at that block.
    - Derive each asset's liability from all non-custody ledger projections and independently cross-check it against the negated custody projections inside one serializable transaction.
    - Persist immutable snapshot, per-asset result, and failure evidence with deterministic identities covering the observed custody amounts and current ledger projection.
    - Treat missing balance projections, invalid customer balances, accounting disagreement, provider uncertainty, stale or regressed finality, wrong bytecode, and a lost in-flight lease as indeterminate.
    - Atomically activate a full asset halt for deficits and indeterminate results; provider-level failures halt every registered deployment asset, surpluses remain explicitly surplus, and no path automatically resolves a halt.
    - Verify balanced, surplus, deficit, indeterminate, empty-account, replay, changed-projection, changed-custody, lease-fencing, failure-idempotency, and database halt-guard behavior against PostgreSQL 16.
    - Keep the worker source-only, opt-in, and unconfigured. Readiness integration, external paging, processor-aware orchestration, daily reports, hosted RPC credentials, deployment, and real-funds claims remain open.
30. [x] **EX-030: Fail-closed local engine recovery integrity**
    - Remove the development/testnet temporary WAL fallback and reject an unavailable configured recovery directory before binding the API listener.
    - Validate every retained WAL segment and record at open and replay time, rejecting truncation, oversized payloads, CRC mismatch, unsupported types, unreadable or missing segments, and duplicate, regressed, or gapped sequences.
    - Serialize sequence allocation with file writes and advance the in-memory sequence only after a successful write so concurrent writers cannot reorder durable records or manufacture a gap after a failed append.
    - Require a matched snapshot/WAL recovery set whenever prior evidence exists; allow a genuinely fresh start only when neither source contains prior evidence.
    - Make snapshot read, parse, version, market/order consistency, next-order sequence, and restoration failures fatal in every runtime mode; migrate the deployed version-1 format only when a clean checkpoint/stop boundary makes its replay sequence unambiguous.
    - Make WAL replay return typed failure context for corrupt or undecodable required entries instead of skipping the record or segment; automatic unclean replay remains disabled until snapshot publication and request/WAL persistence share an atomic boundary.
    - Verify unavailable initialization, CRC corruption, truncation, oversized records, sequence gaps, terminal clean-shutdown semantics, concurrent append ordering, missing/corrupt/unsupported snapshots, invalid order-market restoration, undecodable replay, and valid recovery.
    - Document an evidence-preserving operator recovery procedure and preserve the ledger-first boundary: WAL/snapshots assist local engine recovery but are not authoritative financial storage.
    - Remaining work includes atomic snapshot/checkpoint publication and request/WAL fencing, automatic unclean recovery, cryptographic snapshot integrity and signing, deployed-code identity, replication, encrypted backups, PITR/lost-machine/in-flight rehearsals, write-failure traffic fencing, readiness health, and paging.

## Parallel nonengineering work

These workstreams can run while Milestones 0 and 1 are underway:

- Engage crypto regulatory counsel.
- Decide direct licensing versus licensed-partner launch strategy.
- Define initial permitted and excluded jurisdictions.
- Begin HSM or MPC custody evaluation.
- Identify external security auditors.
- Identify two or more potential market makers.
- Define an initial audit, legal, infrastructure, insurance, and bug-bounty budget.

Provider contracts should not be signed until the custody model and legal launch strategy are chosen.

## Change-management rules

- Every change touching balances, custody, deposits, trades, fees, or withdrawals must include invariant-focused tests.
- Every production fallback must be explicit, documented, observable, and safe. Silent financial fallbacks are prohibited.
- No implementation may infer production behavior from a missing environment variable.
- No new market or leveraged product work begins while a launch gate is red.
- Documentation changes ship with behavior changes.
- Deployment configuration is reviewed as code.
- No mainnet deployment, contract broadcast, secret rotation, commit, push, or public release occurs without explicit approval.
