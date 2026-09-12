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
- [ ] Journal fees and maker rebates explicitly.
- [ ] Add auditable administrative adjustment transactions.
- [ ] Require dual approval for production administrative adjustments.
- [ ] Build deterministic replay from the journal.
- [ ] Integrate the matcher so accepted trades produce atomic ownership transfers.
- [ ] Prevent the matcher from acknowledging a financially effective result before the ledger commits.
- [ ] Remove the engine's role as authoritative owner of customer funds.

### M2 acceptance criteria

- [ ] Property tests prove every journal transaction balances.
- [x] Concurrent operations cannot overspend an account.
- [x] Duplicate requests return the original result without applying twice.
- [ ] A full ledger replay reproduces all materialized balances exactly.
- [ ] A completed trade moves base, quote, fees, and rebates atomically.
- [ ] Engine restart does not change customer ownership.

## Milestone 3: verified deposit pipeline

**Goal:** Credit only finalized, independently verified custody events.

**Target duration:** 1 to 2 engineering weeks.

### M3 tasks

- [ ] Build a dedicated chain indexer for the selected launch chain.
- [ ] Observe the correct contract and deposit event signature.
- [ ] Record chain ID, contract, block number, block hash, transaction hash, transaction index, and log index.
- [ ] Verify depositor, beneficiary, asset, and actual received amount.
- [ ] Implement pending, confirming, finalized, failed, quarantined, and reversed states.
- [ ] Define confirmation thresholds per asset and chain.
- [ ] Detect and process chain reorganizations.
- [ ] Make event identity unique and idempotent.
- [ ] Journal final deposit credits through the ledger.
- [ ] Add redundant RPC providers.
- [ ] Detect stale providers and provider disagreement.
- [ ] Expose deposit progress and evidence to the customer API.
- [ ] Update the frontend to submit or discover the real transaction hash.
- [ ] Remove the invented transaction-hash path.

### M3 acceptance criteria

- [ ] No API request can credit funds without a verified chain event.
- [ ] The same log cannot be credited twice.
- [ ] Reorganizations do not leave invalid finalized credits.
- [ ] Provider failure cannot silently produce a credit.
- [ ] Customers can see pending and finalized deposit status.
- [ ] Custody and ledger amounts reconcile after every finalized deposit.

## Milestone 4: atomic withdrawal pipeline

**Goal:** Make every asset movement out of custody correspond exactly to ledger ownership.

**Target duration:** 1 to 2 engineering weeks.

### M4 tasks

- [ ] Authenticate the customer withdrawal request.
- [ ] Validate available balance and locked exposure.
- [ ] Atomically reserve the ledger amount.
- [ ] Persist the withdrawal before requesting a signature.
- [ ] Bind signing authorization to chain, contract, user, destination, asset, amount, nonce, and request ID.
- [ ] Replace the environment-held signing key with an HSM or MPC adapter boundary.
- [ ] Track requested, reserved, approved, signed, submitted, confirmed, failed, canceled, and reversed states.
- [ ] Observe transaction replacement and dropped transactions.
- [ ] Restore reservations exactly once after permanent failure.
- [ ] Implement ETH and USDC withdrawals through the same state machine.
- [ ] Add per-user, per-asset, and platform withdrawal limits.
- [ ] Add a global withdrawal halt.
- [ ] Add risk and compliance policy hooks without hardcoding a specific vendor.
- [ ] Expose actionable withdrawal status to the frontend.

### M4 acceptance criteria

- [ ] Custody cannot release funds without a matching durable reservation.
- [ ] A failed withdrawal cannot permanently destroy customer funds.
- [ ] Replayed requests and signatures cannot withdraw twice.
- [ ] Concurrent withdrawals cannot exceed available balance.
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

- [ ] Build continuous per-asset custody-to-liability reconciliation.
- [ ] Persist immutable reconciliation snapshots.
- [ ] Automatically halt affected deposits, trading, or withdrawals when required.
- [ ] Add independent reconciliation execution outside the matcher process.
- [ ] Remove temporary WAL fallback behavior.
- [ ] Fail closed on corrupt or unavailable financial storage.
- [ ] Replicate ledger and journal storage across failure domains.
- [ ] Create encrypted offsite backups.
- [ ] Implement point-in-time recovery.
- [ ] Add leader fencing for engine failover.
- [ ] Test recovery while deposits, orders, trades, and withdrawals are in flight.
- [ ] Define and measure RPO and RTO.
- [ ] Add readiness checks for ledger, indexer, signer, custody RPC, and reconciler.
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
- [ ] Complete ETH and USDC deposit status interfaces.
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
13. [ ] **EX-013: Managed PostgreSQL and runtime integration**
    - Select and provision the managed production and staging PostgreSQL service.
    - Add database roles, secret injection, readiness, pooling, backup policy, and deployment migrations before connecting the API or matcher.

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
