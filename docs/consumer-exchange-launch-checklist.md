# Vela Consumer Exchange Launch Checklist

This is the master checklist for turning Vela from its current public beta into a real consumer crypto exchange. It includes product decisions, removals, implementation work, compliance, security, operations, liquidity, testing, and deployment.

The ordered execution plan is documented in the [consumer exchange engineering plan](consumer-exchange-engineering-plan.md).

## Product and legal structure

- [ ] Decide whether Vela will be:
  - [ ] A regulated custodial exchange with an authoritative off-chain ledger
  - [ ] A genuinely trust-minimized DEX or rollup with enforceable on-chain settlement
  - [ ] A hybrid exchange operated through licensed custody and compliance partners
- [ ] Define exactly which entity operates Vela.
- [ ] Define where that entity is incorporated and physically operated.
- [ ] Define the initial launch jurisdictions.
- [ ] Decide whether U.S. residents will be permitted.
- [ ] Decide whether California residents will be permitted.
- [ ] Decide whether New York residents will be permitted.
- [ ] Decide whether the initial launch is consumer, institutional, or invite-only.
- [ ] Decide whether Vela will custody assets or use a qualified or licensed custody partner.
- [ ] Decide whether Vela will ever support fiat.
- [x] Decide whether Vela will support only spot markets at launch.
- [x] Keep perps out of the initial consumer launch.
- [x] Keep retail leverage and margin out of the initial launch.
- [x] Keep lending and borrowing out of the initial launch.
- [x] Keep public LP and MM vaults out of the initial launch.
- [x] Keep reputation-based credit out of the initial launch.
- [x] Keep permissionless token listings out of the initial launch.
- [x] Keep bridge-attested deposits out until the direct deposit system is secure.
- [ ] Engage specialist U.S. crypto regulatory counsel.
- [ ] Obtain a written legal analysis of Vela's custody and exchange model.
- [ ] Obtain a written legal analysis for every initially listed asset.
- [ ] Determine whether Vela must register with FinCEN as an MSB.
- [ ] Complete FinCEN MSB registration if required.
- [ ] Establish a Bank Secrecy Act compliance program.
- [ ] Determine applicable state money-transmission licenses.
- [ ] Determine California DFAL requirements before serving Californians.
- [ ] Determine New York BitLicense or trust-charter requirements before serving New Yorkers.
- [ ] Determine licensing requirements in every supported state.
- [ ] Determine applicable international licensing requirements.
- [ ] Establish a formal restricted-jurisdiction policy.
- [ ] Implement geographic restrictions based on the approved jurisdiction list.
- [ ] Establish a policy for VPN, proxy, and sanctions-evasion behavior.
- [ ] Determine whether any feature creates broker-dealer, exchange, ATS, or clearing-agency obligations.
- [ ] Obtain securities-law review for each token before listing it.
- [ ] Obtain CFTC review before offering perps, futures, options, margin, or financed commodity transactions.
- [ ] Document Vela's regulatory assumptions and update them when the product changes.

## Deferred exchange-token decision

- [x] Keep any Vela exchange token out of the initial launch and current
      infrastructure-remediation critical path.
- [ ] Treat `$VELA` and `$VCE` only as working labels until ticker, trademark,
      domain, chain, and existing-token collision diligence is complete.
- [ ] Resolve the known existing Vela Exchange (`VELA`) token collision; do not
      adopt or market `$VELA` while customer-confusion and trademark risks remain.
- [ ] Define the exact problem a transferable token would solve.
- [ ] Compare the proposed token against loyalty points, volume tiers, fee
      credits, rebates, and ordinary equity financing.
- [ ] Obtain written jurisdiction-specific legal analysis for issuance, sale,
      airdrops, rewards, staking, custody, transfer, secondary trading, and
      self-listing before writing production token contracts.
- [ ] Select and document the issuing entity and every related-party conflict.
- [ ] Design and stress-test supply, allocations, vesting, unlocks, emissions,
      treasury powers, burns or buybacks, liquidity, and fully diluted value.
- [ ] Establish insider, treasury, market-maker, surveillance, disclosure,
      independent-listing, and delisting controls.
- [ ] Define tax, accounting, reporting, AML/KYC, sanctions, record-retention,
      and customer-support obligations.
- [ ] Define genuine and bounded governance utility, or omit governance claims.
- [ ] Define explicit criteria for choosing points, fee tiers, or no token.
- [ ] Keep exchange solvency, ordinary deposits, trading, withdrawals, customer
      recovery, and company runway independent of the token and its market price.
- [ ] Commission independent economic, legal, smart-contract, and security
      reviews before public distribution or exchange listing.
- [ ] Complete a testnet rehearsal and capped launch before broad distribution.

## Critical custody and settlement redesign

- [x] Restrict the current unverified direct-credit endpoint to `testnet-demo`.
- [x] Label every response from that endpoint as an unverified demo credit.
- [ ] Disable the current unauthenticated `POST /deposit` crediting path.
- [ ] Stop accepting a user-provided address, asset, and amount as sufficient evidence of a deposit.
- [ ] Stop generating synthetic transaction hashes for deposits.
- [x] Require the real chain ID and transaction hash for every authoritative deposit observation; the testnet-only synthetic demo endpoint remains separately contained.
- [x] Verify that the transaction log came from the immutable-registry settlement contract.
- [x] Verify that the pinned `Deposited(bytes32,address,address,uint256)` event was emitted.
- [x] Verify the indexed depositor/beneficiary address for the current self-deposit contract flow.
- [x] Verify the indexed asset address against the ledger asset registry.
- [x] Verify the actual amount received; ERC20 custody now measures and emits the vault balance delta.
- [x] Wait for the asset-and-chain finality policy before final credit; Base ETH and USDC require two-provider agreement at the highest common `finalized` height.
- [x] Handle chain reorganizations by continuously re-auditing already-processed finalized history through two agreeing providers and a guarded cyclic checkpoint.
- [x] Reverse or quarantine deposits invalidated by a reorganization; remaining available funds move to customer pending, exact shortfalls are recorded, and the affected asset is halted first.
- [x] Make finalized deposit crediting idempotent at the ledger boundary.
- [x] Prevent the same canonical transaction log from being recorded or credited twice at the ledger boundary.
- [x] Store chain ID, vault, block number, block hash, transaction hash, transaction index, and log index for every authoritative deposit observation.
- [x] Complete observed, confirming, finalized, credited, rejected, orphaned, quarantined, and reversed deposit workflows with guarded state history and compensating journals where funds can be isolated.
- [x] Build a dedicated deposit indexer with restart-safe PostgreSQL checkpointing and a single-worker lease.
- [x] Use redundant RPC providers with distinct configured names and origins.
- [x] Add RPC disagreement, wrong-chain, stale-head, provider-lag, finalized-regression, and pinned-bytecode detection that halts credits.
- [x] Decide which ledger is authoritative.
- [x] Make PostgreSQL balance projections authoritative for customer REST and WebSocket reads; matching-engine balances are only a replayed execution cache when the ledger is configured.
- [x] Fail closed on every known legacy API route that directly mutates matching-engine balances while the authoritative ledger is configured.
- [ ] Complete integration of the double-entry financial ledger; matcher reservations, releases, trades, fees, rebates, referral payouts, deposits, withdrawal completion, and custody reconciliation have typed paths when configured, while full withdrawal failure handling, recovery, and processor-aware orchestration remain open.
- [x] Journal every authoritative finalized deposit atomically with its credited state; unverified demo credits remain testnet-only and non-authoritative.
- [x] Journal every matcher-produced trade when the authoritative ledger is configured.
- [x] Journal every matcher-produced fee when the authoritative ledger is configured.
- [x] Journal every matcher-produced rebate when the authoritative ledger is configured.
- [x] Journal every withdrawal reservation atomically before acknowledging it; identical retries return the original journal transaction and concurrent reservations cannot overspend.
- [x] Journal every completed withdrawal atomically with matching finalized V2 execution evidence; the implementation remains source- and test-only.
- [ ] Journal every failed or reversed withdrawal; finalized V2 expiry now journals an exactly-once release, but other failure and reversal paths remain open.
- [x] Journal every administrative adjustment through the typed dual-control correction path.
- [x] Require balanced debits and credits for every journal transaction at the PostgreSQL commit boundary.
- [ ] Make ledger writes durable before acknowledging every financial operation; matcher operations now satisfy this, but deposits and withdrawals remain open.
- [ ] Give every financial operation a globally unique idempotency key.
- [ ] Prevent test, synthetic, bridge, and real-money balances from mixing.
- [ ] Add separate environments and ledger namespaces for testnet and production.
- [x] Provision controlled per-asset custody-vault counter-accounts for ETH and USDC during ledger bootstrap.
- [x] Reconcile each registered chain asset independently in the opt-in worker; deployed RPC end-to-end validation remains open.
- [x] Continuously compare finalized custody assets against total ledger liabilities with an independent custody-account cross-check; the worker remains unconfigured and stopped by default.
- [x] Halt deposit credits, trading, and new withdrawals automatically on deficits or indeterminate reconciliation evidence; operators must resolve halts through the existing dual-control path.
- [ ] Generate daily proof-of-liabilities snapshots.
- [ ] Generate daily reconciliation reports.
- [x] Preserve reconciliation results in an immutable PostgreSQL history table.

## Settlement-contract architecture

- [ ] Replace the current disconnected per-user deposit accounting model.
- [ ] Ensure trades change the balances users can ultimately withdraw.
- [ ] Prevent a seller from emergency-exiting an asset after selling it.
- [ ] Ensure a buyer can withdraw an asset acquired through trading.
- [x] Decide between:
  - [x] An omnibus custody vault backed by an authoritative off-chain ledger
  - [ ] An on-chain committed post-trade balance tree
  - [ ] A validity-proof settlement system
  - [ ] An optimistic settlement system with enforceable challenges
- [ ] If using an off-chain ledger, define the legal and technical custody model explicitly.
- [ ] If using committed balances, make withdrawals derive from the latest finalized post-trade state.
- [ ] Implement a finalized state-root format that includes all user balances.
- [ ] Include deposits, trades, fees, rebates, withdrawals, and nonces in the committed state.
- [ ] Define deterministic transition rules for every state mutation.
- [ ] Implement forced transaction inclusion.
- [ ] Implement a real delayed inbox.
- [ ] Allow users to submit deposits and withdrawals through the forced-inclusion path.
- [ ] Enforce the inclusion deadline on-chain.
- [ ] Define the system response when the operator misses the deadline.
- [ ] Implement enforceable fraud proofs or validity proofs.
- [ ] Implement operator bonding and slashing if using optimistic proofs.
- [ ] Make the emergency-exit mechanism use finalized post-trade balances.
- [ ] Define how emergency exits work when data availability fails.
- [ ] Define how emergency exits work during chain reorganizations.
- [ ] Define how emergency exits work while the contract is paused.
- [ ] Define how partially settled batches are handled.
- [ ] Define how failed withdrawals restore ledger availability.
- [ ] Prevent simultaneous ordinary withdrawal and emergency-exit claims.
- [ ] Add an explicit on-chain asset allowlist.
- [ ] Reject unsupported and fee-on-transfer assets.
- [ ] Credit token deposits using the actual balance increase, not the requested transfer amount.
- [ ] Reassess whether contracts should be immutable.
- [ ] If contracts remain immutable, create a safe migration mechanism.
- [ ] Define how customer claims move from a compromised contract to a replacement.
- [ ] Build contract-to-engine integration tests.
- [ ] Test deposit, trade, withdrawal, and emergency exit as one complete flow.
- [ ] Test buyers withdrawing acquired assets.
- [ ] Test sellers being unable to reclaim sold assets.
- [ ] Test insolvency and reconciliation invariants across the complete system.

## Withdrawal system

- [x] Disable production withdrawal signing until durable ledger reservation exists.
- [x] Replace the authoritative path's independent withdrawal-signature flow with a reservation-bound signer adapter; retain the legacy signer only for non-authoritative demos.
- [x] Require authenticated customer authorization before generating a withdrawal reservation.
- [x] Atomically reserve the withdrawal amount in the ledger.
- [x] Check available balance before signing.
- [ ] Check open-order and locked-balance exposure before signing.
- [x] Require a durable provider-neutral compliance disposition before authorization and signing; `reject`, `review`, and provider unavailability cannot produce a signature, while an external screening provider remains unconfigured.
- [ ] Check sanctions status before signing.
- [x] Check exact per-asset single-withdrawal caps plus customer amount/count and platform amount velocity windows before authorization.
- [x] Store the withdrawal request before producing a custody signature.
- [x] Bind custody authorization to chain ID, vault contract, user, destination, asset code and token address, amount, nonce, expiry, policy, approver, and request ID.
- [x] Mark signatures as issued in immutable PostgreSQL evidence and prevent duplicate issuance per withdrawal and authorization.
- [ ] Broadcast withdrawals through a controlled service or reliably observe customer broadcasts; EX-027 can observe already-recorded attempts when explicitly configured, but production broadcasting and customer-relay discovery remain disabled.
- [ ] Track submitted, pending, confirmed, failed, replaced, and dropped transactions; the opt-in worker now persists exact finalized reverts and verifies supplied replacement hashes, but automatic replacement discovery and a production relay or hash feed remain open.
- [x] Restore reserved funds exactly once after a permanent V2 expiry, only when finalized two-provider evidence proves the request ID and nonce unused, no execution event, and every attempt terminal.
- [ ] Prevent an on-chain withdrawal from succeeding without the corresponding ledger debit.
- [x] Prevent a completion debit before finalized matching chain execution and restore the reservation after finalized V2 expiry non-execution proof; ambiguous transactions remain reserved.
- [ ] Implement complete ERC-20 withdrawal support.
- [ ] Remove the manual "processed within 24 hours" ERC-20 workflow.
- [x] Add immutable per-deployment withdrawal confirmation policies requiring finalized evidence, two distinct providers, and a configurable positive confirmation minimum.
- [x] Add versioned per-user, per-asset, and platform-wide withdrawal limits in atomic asset units.
- [ ] Add cooling-off periods for suspicious account changes.
- [ ] Add large-withdrawal review thresholds.
- [ ] Add a withdrawal allowlist option.
- [x] Add a durable global withdrawal kill switch that blocks new reservation, approval, and signing while allowing safe releases and mandatory journaling of already-finalized custody execution; require three-party evidence to resolve it.
- [x] Add clear authoritative customer-visible withdrawal phase, funds disposition, terminality, cancellation eligibility, ledger receipts, and operator-review guidance; actual operator recovery execution remains an operations gate.

## Remove test and synthetic behavior from production

- [x] Remove unconditional synthetic user funding from production startup.
- [x] Remove unconditional synthetic market seeding from production startup.
- [x] Prevent the `0x...01` seed account from existing in production state.
- [x] Prevent the internal bot from trading with invented collateral.
- [x] Add an explicit environment type rather than relying only on an environment string.
- [x] Make production startup reject all test fixtures.
- [x] Make production startup reject testnet contract addresses.
- [x] Make production startup reject Sepolia chain ID.
- [x] Make production startup reject placeholder RPC URLs.
- [x] Make production startup reject demo Alchemy URLs.
- [x] Make production startup reject mock provers.
- [x] Make production startup reject simulated TEE attestations.
- [x] Make production startup reject writable temporary storage.
- [x] Add tests proving production mode cannot seed balances.
- [x] Add tests proving production mode cannot start with testnet configuration.

## Persistence and financial data integrity

- [x] Stop falling back to a temporary WAL directory in every runtime mode.
- [x] Fail closed if the configured WAL cannot be initialized or validated.
- [x] Stop starting from a fresh seeded state when snapshot restoration fails; a fresh start now requires no prior snapshot or WAL evidence.
- [x] Fail closed when the local engine snapshot or WAL recovery set is corrupt; PostgreSQL remains the authoritative financial journal when configured.
- [x] Require an explicit, evidence-preserving operator recovery procedure for corrupted or incomplete local engine state.
- [x] Gate committed sharded matching mutations and caller success on a typed local WAL batch append and fsync; PostgreSQL/local-WAL cross-resource atomicity remains unavailable.
- [x] Serialize committed sharded-state snapshot/checkpoint publication against matching mutation publication and sync the snapshot file and directory before checkpointing.
- [ ] Store the financial journal in replicated durable storage.
- [x] Treat matching-engine customer balances and platform fees as a cache rebuilt from the authoritative journal whenever the ledger is configured.
- [ ] Rebuild complete engine state from authoritative records; financial balances and fees replay now, while complete order-book and nonfinancial state recovery still depends on snapshots/WAL.
- [x] Add SHA-256 integrity metadata for the snapshot and every WAL segment in each immutable recovery bundle; raw WAL records retain CRC64 for local corruption detection.
- [x] Sign exact recovery-manifest bytes with a dedicated secp256k1 key and require a separately pinned verification key before export, import, or restore.
- [x] Store snapshot format, exact deployed build identity, leadership generation, WAL boundary, byte lengths, and SHA-256 hashes in the recovery manifest.
- [x] Bind every new ledger-backed engine snapshot and signed recovery manifest to an immutable authoritative-journal commit sequence and exact transaction identity; restored startup rejects an older, divergent, or projection-invalid database.
- [x] Verify snapshot format compatibility and internal market/order sequencing before loading, and bind each recovery bundle to an exact deployed build identity.
- [ ] Replicate WAL and ledger data across failure domains.
- [ ] Create encrypted offsite backups.
- [x] Stream signed recovery bundles into authenticated age/X25519 archives through a provider-neutral atomic export/import boundary; actual separately provisioned offsite storage and retention remain open.
- [x] Define recovery point objectives.
- [x] Define recovery time objectives.
- [x] Test PostgreSQL 16 physical base-backup plus archived-WAL recovery to a named restore point in an isolated container; managed-provider PITR rehearsal remains open.
- [ ] Test recovery from a lost engine machine.
- [x] Test a clean, cryptographically verified restore after removing the original local engine directory; a hosted lost-machine rehearsal remains open.
- [x] Test fail-closed handling of corrupt and unsupported local engine snapshots.
- [x] Test fail-closed handling of incomplete, oversized, checksum-invalid, and sequence-invalid local WAL records.
- [ ] Test recovery while chain deposits or withdrawals are in flight.
- [x] Test the cross-resource recovery selector with a later in-flight journal commit and retain the exact earlier engine watermark; a combined hosted chain-worker disaster exercise remains open.
- [x] Add PostgreSQL generation fencing so two ledger-backed engine leaders cannot commit matching mutations simultaneously; no-ledger demo operation remains explicitly single-process only.
- [x] Define an active-passive or consensus-backed availability design.
- [ ] Preserve regulatory records for the legally required retention period.
- [ ] Make audit records tamper-evident.
- [x] Give administrative corrections a dual-approval process.
- [ ] Authenticate correction initiators and approvers with separate scoped operator identities before exposing an operator API.

## Security and key management

- [ ] Commission an external smart-contract audit after redesigning settlement.
- [x] Enforce the initial ETH/USDC scope with an on-chain allowlist and reject fee-on-transfer deposits without blocking emergency exit after delisting.
- [x] Require reproducible deployment inputs and emit source-commit, creation-bytecode, runtime-template, and final on-chain code-hash evidence; actual deployed artifacts still require review and signature.
- [x] Add a canonical-USDC mainnet-fork harness; a hosted run with an authenticated RPC remains open.
- [ ] Commission an external Rust engine and API security review.
- [ ] Commission a frontend and wallet-interaction security review.
- [ ] Commission an independent penetration test.
- [ ] Fix every Critical and High finding.
- [ ] Freeze the audited release at a specific commit.
- [ ] Publish the final audit report.
- [ ] Publish the audited commit hash.
- [ ] Launch a funded public bug bounty.
- [ ] Move the operator signing key out of an environment variable.
- [ ] Use an HSM or MPC-backed signing service.
- [ ] Define signing policies inside the HSM or MPC system.
- [ ] Require multiple approvals for key rotation.
- [ ] Put contract ownership under a real multisig.
- [ ] Use separate physical devices for owner, operator, and guardian roles.
- [ ] Define backup and recovery procedures for each key.
- [ ] Define what happens if a signer dies, disappears, or loses access.
- [ ] Test operator-key rotation.
- [ ] Test guardian-key rotation.
- [ ] Test multisig signer replacement.
- [ ] Add withdrawal-signing rate limits.
- [x] Require the active immutable policy version and matching approved decision at the PostgreSQL authorization and signing boundaries.
- [ ] Add anomalous signing alerts.
- [ ] Store a tamper-evident log of every signature request and result.
- [ ] Require code review for changes to authentication, ledger, withdrawal, and custody code.
- [x] Make dependency security scanning blocking.
- [x] Remove `continue-on-error` from `cargo-audit`.
- [x] Make secret scanning blocking.
- [x] Remove `continue-on-error` from gitleaks.
- [x] Add frontend dependency auditing.
- [ ] Add Solidity static analysis.
- [ ] Add fuzzing for the Rust API boundary.
- [ ] Add fuzzing for deposit and withdrawal parsers.
- [ ] Add fuzzing for chain event decoding.
- [ ] Add supply-chain controls and dependency pinning.
- [ ] Generate software bills of materials.
- [ ] Sign release artifacts and container images.
- [ ] Restrict production deploy permissions.
- [ ] Require protected branches and deployment approvals.
- [ ] Implement least-privilege access for Fly, Vercel, RPC providers, and monitoring.
- [ ] Establish a credential-rotation schedule.
- [x] Never expose admin credentials through `NEXT_PUBLIC_*` variables.
- [x] Remove stale `NEXT_PUBLIC_ADMIN_TOKEN` entries from source, ignored local files, and deployed Vercel environment configuration.
- [x] Replace frontend admin API access with a public, sanitized reserves endpoint.
- [ ] Use constant-time token verification for any remaining shared-secret controls.
- [ ] Prefer short-lived, scoped administrative credentials over one global token.

## Authentication and customer privacy

- [x] Require authentication for customer balance endpoints.
- [x] Require authentication for open-order endpoints.
- [x] Require authentication for portfolio endpoints.
- [x] Require authentication for CSV and tax exports.
- [x] Require authentication for private fill history.
- [x] Verify that a wallet can only access its own private HTTP records.
- [x] Add authorization tests for every customer-data endpoint.
- [x] Reconcile HTTP authentication with WebSocket authentication by reusing the short-lived account session.
- [ ] Align frontend WebSocket event envelopes with the backend schema.
- [ ] Make WebSocket reconnect and replacement-socket transitions race-safe.
- [x] Use server-issued, single-use challenges.
- [x] Expire authentication challenges.
- [x] Rate-limit challenge issuance per account; add ingress-level IP and device abuse controls before production.
- [x] Prevent challenge replay across protocol domains and runtime environments.
- [ ] Implement session keys so users do not approve every order manually.
- [ ] Add session-key creation UI.
- [ ] Add session-key revocation UI.
- [ ] Add expiration and per-market scopes.
- [ ] Add order-side and order-type scopes.
- [ ] Add per-order and cumulative notional limits.
- [ ] Add on-chain or independently enforceable revocation.
- [ ] Add compromised-session emergency revocation.
- [ ] Support WalletConnect and mobile wallets.
- [ ] Test MetaMask, Rabby, Coinbase Wallet, Brave, and WalletConnect.
- [ ] Remove direct `window.ethereum` usage where wagmi or viem can provide typed flows.
- [x] Avoid storing sensitive authentication material in local storage.
- [x] Define session timeout and reauthentication behavior: 30-minute in-memory sessions, explicit sign-out revocation, and a fresh signature after refresh or expiry.
- [ ] Move account challenge and session state into a shared, revocation-capable store before running multiple API replicas.

## Compliance program

- [ ] Appoint a qualified BSA and AML compliance officer.
- [ ] Conduct and document an enterprise AML risk assessment.
- [ ] Implement customer identification and KYC.
- [ ] Implement identity-document verification where required.
- [ ] Implement beneficial-owner checks for business accounts.
- [ ] Implement sanctions screening at onboarding.
- [ ] Implement continuous sanctions rescreening.
- [ ] Implement wallet-address sanctions screening.
- [ ] Implement transaction-risk scoring.
- [ ] Implement source-of-funds review for high-risk customers.
- [ ] Implement enhanced due diligence for high-risk accounts.
- [ ] Implement suspicious-activity monitoring.
- [ ] Implement case management for compliance alerts.
- [ ] Establish SAR investigation and filing workflows.
- [ ] Establish currency-transaction reporting workflows if applicable.
- [ ] Implement Travel Rule workflows where applicable.
- [ ] Implement record-retention requirements.
- [ ] Train relevant employees and contractors.
- [ ] Conduct independent AML-program testing.
- [ ] Establish blocked-property procedures.
- [ ] Establish OFAC reporting procedures.
- [ ] Maintain records of blocked digital assets.
- [ ] Define prohibited customers, jurisdictions, and transaction types.
- [ ] Implement account freezes that preserve customer and regulator records.
- [ ] Add law-enforcement request procedures.
- [ ] Add subpoena and information-request procedures.
- [ ] Add privacy-request and data-deletion procedures consistent with financial-record obligations.
- [ ] Establish a formal token-listing committee.
- [ ] Establish a written listing standard.
- [ ] Review securities, commodities, sanctions, technical, liquidity, and concentration risks for each asset.
- [ ] Establish token-monitoring and delisting procedures.

## Market integrity and risk controls

- [ ] Establish a market-surveillance program.
- [ ] Detect wash trading across linked wallets and accounts.
- [ ] Detect spoofing.
- [ ] Detect layering.
- [ ] Detect quote stuffing.
- [ ] Detect pump-and-dump activity.
- [ ] Detect self-dealing involving the operator or internal market maker.
- [ ] Preserve raw order and cancellation records for investigations.
- [ ] Preserve deterministic market-event sequencing.
- [ ] Add account-linkage analysis.
- [ ] Add IP, device, funding-source, and wallet-cluster signals where legally permitted.
- [ ] Add price collars.
- [ ] Add fat-finger checks.
- [ ] Add maximum order-value controls.
- [ ] Add maximum position and exposure controls.
- [ ] Add per-market circuit breakers.
- [ ] Add exchange-wide circuit breakers.
- [ ] Add cancel-on-disconnect.
- [ ] Add mass-cancel functionality.
- [ ] Add an operator kill switch.
- [ ] Define stale-oracle behavior.
- [ ] Define crossed or unavailable reference-market behavior.
- [ ] Define chain-congestion behavior.
- [ ] Define market reopening procedures.
- [ ] Establish error-trade and trade-bust policies.
- [ ] Establish fair-access and order-priority policies.
- [ ] Publish clear maker and taker fee schedules.
- [ ] Define rebate accounting and funding.
- [ ] Validate that negative maker fees cannot create undercollateralized liabilities.
- [ ] Disable MM credit until collateral and loss allocation are production-ready.
- [ ] Define how MM losses are absorbed.
- [ ] Define default waterfalls.
- [ ] Define exposure caps per market maker.
- [ ] Require approved market makers to maintain funded accounts.
- [ ] Monitor spread, depth, uptime, and execution quality.
- [ ] Detect toxic or stale quoting.
- [ ] Establish formal market-maker agreements.
- [ ] Prevent the operator's internal MM from trading on privileged information.
- [ ] Separate exchange operations from proprietary market making.
- [ ] Publish conflicts-of-interest disclosures.

## Liquidity and asset support

- [ ] Replace synthetic order books with real, funded liquidity.
- [ ] Contract with multiple independent market makers.
- [ ] Avoid dependence on a single market maker.
- [ ] Avoid dependence on CoinGecko as the sole reference source.
- [ ] Use multiple high-quality price sources.
- [ ] Add source-disagreement detection.
- [ ] Add freshness thresholds.
- [ ] Add per-source health monitoring.
- [x] Define which wrapped representation of each asset is traded.
- [ ] Do not represent native BTC, SOL, or other non-EVM assets as interchangeable with arbitrary internal symbols.
- [ ] Define custody and withdrawal rails for every supported asset.
- [x] Start with ETH and USDC or another very small market set.
- [ ] Expand only after deposit, trade, and withdrawal invariants are proven.
- [ ] Define minimum liquidity requirements before listing a market.
- [ ] Define maximum market concentration.
- [ ] Define emergency delisting behavior.
- [ ] Define how open orders are canceled during delisting.
- [ ] Define how customers withdraw delisted assets.

## Verifiability and public claims

- [x] Correct claims that every batch is presently proven on-chain.
- [x] Correct claims that current fraud proofs are enforceable.
- [x] Correct claims that users can always withdraw without operator permission.
- [x] Correct claims that all trades are presently settled on-chain.
- [x] Correct claims that the system is trustless.
- [x] Clearly distinguish testnet behavior from planned mainnet behavior.
- [x] Clearly distinguish placeholder proofs from real proofs.
- [x] Clearly distinguish simulated TEE records from hardware attestations.
- [x] Ensure the homepage, README, application, and documentation describe the same trust model.
- [ ] Publish the actual operator powers.
- [ ] Publish the actual guardian powers.
- [ ] Publish the actual owner powers.
- [x] Publish current settlement limitations.
- [x] Publish current data-availability limitations.
- [x] Publish current forced-inclusion limitations.
- [ ] Publish what happens if the operator disappears.
- [ ] Publish what happens if the engine loses state.
- [ ] Publish what happens if custody becomes undercollateralized.
- [ ] Implement real SP1 proving before describing batches as ZK-proven.
- [ ] Validate proofs against a fixed, audited guest program.
- [ ] Bind proofs to the deployed matcher version.
- [ ] Make proof verification independently reproducible.
- [ ] Implement on-chain verification if that remains part of the product claim.
- [ ] Implement real data availability.
- [ ] Make full batch inputs available to independent verifiers.
- [ ] Define retention and availability guarantees.
- [ ] Implement real TEE attestation only on supported hardware.
- [ ] Make production fail closed if required verification infrastructure is missing.
- [ ] Avoid requiring TEE and ZK infrastructure purely for marketing if they are not part of the chosen security model.

## Production infrastructure and operations

- [x] Confirm why the live engine health and status endpoints were timing out and recover the testnet-demo service.
- [x] Inspect the Fly application through its control plane and document the incident evidence.
- [ ] Add external uptime monitoring.
- [ ] Add internal health monitoring.
- [x] Separate process liveness and service readiness endpoints.
- [x] Include ledger connectivity and exact schema version in readiness.
- [x] Include matcher storage, engine leadership, recovery replication, chain indexer, signer, custody RPC, and reconciler health in readiness; production requires redacted bounded endpoints for all dependencies.
- [x] Report redacted PostgreSQL engine-leadership state and generation in readiness/status and separate configured/active metrics without exposing the lease owner identity.
- [x] Report redacted recovery-replication health, last success, checkpoint sequence, and failures without exposing filesystem paths.
- [x] Report only redacted recovery-signing and encrypted-offsite configuration state without exposing keys, recipients, or destinations.
- [x] Latch a process-lifetime matcher-storage fence after ledger/WAL/snapshot durability failure, reject current and queued matching proposals, and reject new non-GET HTTP traffic while preserving liveness diagnostics.
- [x] Prevent the service from accepting traffic while local recovery or authoritative ledger replay is incomplete by completing both before binding the listener.
- [x] Add provider-neutral state-change alerts for failed WAL writes through the latched matcher-storage health boundary; a real paging receiver remains to be provisioned.
- [x] Add provider-neutral state-change alerts for failed snapshots and recovery replication; a real paging receiver remains to be provisioned.
- [ ] Add alerts for reconciliation differences.
- [ ] Add alerts for delayed deposits.
- [ ] Add alerts for stuck withdrawals.
- [ ] Add alerts for unusual withdrawal volume.
- [x] Add provider-neutral readiness and state-change alerts for signer and custody-RPC failures; a real paging receiver remains to be provisioned.
- [ ] Add alerts for stale market data.
- [ ] Add alerts for market-maker disappearance.
- [ ] Add alerts for placeholder prover or TEE activation.
- [ ] Add alerts for custody-ratio deterioration.
- [ ] Establish 24/7 incident ownership before accepting meaningful funds.
- [ ] Create an on-call rotation.
- [ ] Define severity levels.
- [ ] Define paging policies.
- [ ] Define escalation paths.
- [ ] Create incident runbooks.
- [ ] Run compromise exercises.
- [ ] Run insolvency exercises.
- [ ] Run chain-reorganization exercises.
- [ ] Run RPC-outage exercises.
- [ ] Run signer-loss exercises.
- [ ] Run database-corruption exercises.
- [ ] Run market-data outage exercises.
- [ ] Run denial-of-service exercises.
- [ ] Define maintenance windows.
- [ ] Define customer notification procedures.
- [ ] Create an independently hosted status page.
- [ ] Preserve incident history outside the engine's local snapshot.
- [ ] Establish post-incident review procedures.
- [ ] Define capacity requirements.
- [ ] Run load tests using real HTTP and WebSocket traffic.
- [ ] Run long-duration soak tests.
- [ ] Test reconnect storms.
- [ ] Test WebSocket sequence gaps.
- [ ] Test backpressure and overloaded queues.
- [ ] Test API and matching-engine saturation.
- [ ] Test mass cancellation.
- [ ] Test large market moves.
- [ ] Test multi-market contention.
- [ ] Test chain and RPC latency spikes.
- [ ] Establish safe deployment and rollback gates.
- [ ] Do not deploy every push to `main` directly to production.
- [ ] Add staging, canary, and production environments.
- [ ] Require production deployment approval.
- [x] Add a controlled database migration procedure with a separate migrator credential.
- [ ] Add protocol and contract migration procedures.
- [ ] Verify frontend, backend, and contract versions are compatible.
- [ ] Publish or internally record every deployed commit hash.

## Mainnet deployment preparation

- [ ] Complete the external audit before mainnet deployment.
- [ ] Address all Critical and High findings.
- [ ] Freeze contracts and backend at known commits.
- [ ] Generate contract deployment artifacts reproducibly.
- [ ] Create the missing `DeployPerp.s.sol` script if perps are ever deployed.
- [ ] Create the missing `DeployVault.s.sol` script if the vault is ever deployed.
- [ ] Create the missing sequencer-registry deployment script.
- [ ] Do not deploy unused contracts on day one.
- [ ] Validate all constructor and role addresses.
- [ ] Validate contract ownership transfer.
- [ ] Validate multisig acceptance of ownership.
- [ ] Validate guardian configuration.
- [ ] Validate operator configuration.
- [ ] Validate asset allowlists.
- [ ] Validate chain-specific oracle and sequencer-feed configuration.
- [ ] Verify source code on the relevant block explorer.
- [ ] Record addresses, chain IDs, bytecode hashes, and commit hashes.
- [ ] Perform small-value deposit, trade, and withdrawal tests.
- [ ] Verify reconciliation before raising limits.
- [ ] Start with strict total-value and per-user caps.
- [ ] Establish an explicit launch rollback decision tree.
- [ ] Prepare customer communications before deployment.
- [ ] Publish the audit, addresses, risks, fees, and supported assets.
- [ ] Verify every production URL and environment variable.
- [ ] Remove all Sepolia references from mainnet UI.
- [ ] Keep a permanent distinction between testnet and mainnet domains.

## Consumer product work

- [x] Build the ledger-backed deposit status UI; it polls exact customer-scoped states and remains hidden when the compatible Base deployment is not configured.
- [ ] Show confirmation progress.
- [x] Show real transaction hashes linked to the configured Base block explorer.
- [x] Show rejected, orphaned, reversed, and quarantined deposits through the same authoritative state list.
- [ ] Build complete withdrawal status UI.
- [x] Build the authoritative withdrawal-reservation UI with exact ETH/USDC amounts, wallet-bound request identity, ledger status polling, and safe pre-signing cancellation.
- [ ] Show withdrawal review and confirmation state.
- [ ] Give customers actionable recovery instructions.
- [ ] Support ERC-20 withdrawals on-chain.
- [ ] Remove manual withdrawal promises.
- [ ] Implement session keys to avoid a wallet popup for every order.
- [ ] Add session-key permissions and revocation UI.
- [ ] Support mobile layouts fully.
- [ ] Support mobile wallets.
- [ ] Support WalletConnect.
- [ ] Test browser and wallet compatibility.
- [ ] Add loading, empty, disconnected, degraded, and error states across every consumer flow; the trading terminal states are complete.
- [x] Distinguish live external reference prices from synthetic Vela testnet order books on market discovery and trading surfaces.
- [x] Remove unverified hard-coded latency and throughput claims from the consumer market interface.
- [x] Make market discovery cards and terminal market selection keyboard-accessible semantic links.
- [x] Add a cohesive, brand-specific visual treatment to the terminal header and token rail without importing third-party branding or presenting invented market, holder, or social metrics.
- [x] Separate terminal chrome into logo, centered search, account controls, inset discovery rail, chart-level asset identity, and execution context regions while retaining mobile navigation access to every displaced route.
- [x] Extend the terminal visual system to the private customer profile with authenticated ledger balances, current marked allocation, positions, PnL, orders, fills, funding activity, and factual testnet leaderboard context without invented social metrics or historical performance curves.
- [x] Surface backend order-rejection explanations in the trading ticket without converting unsupported order semantics into misleading controls.
- [x] Add authoritative maker/taker tier fee previews before order submission.
- [ ] Add balance, available, locked, and pending distinctions.
- [ ] Add deposit and withdrawal limits to the UI.
- [ ] Add account statements.
- [ ] Add complete fill history.
- [ ] Add PnL and cost-basis reporting.
- [ ] Add CSV export.
- [ ] Add tax-lot export.
- [ ] Add transaction receipts.
- [ ] Add downloadable customer records.
- [ ] Add customer-support contact information.
- [ ] Add complaint intake and tracking.
- [ ] Add account-compromise reporting.
- [ ] Add accessibility testing.
- [ ] Add end-to-end browser testing.
- [ ] Add wallet-interaction integration testing.
- [x] Increase frontend test coverage beyond the original single smoke test with verified-deposit configuration safety tests.
- [ ] Test deposits and withdrawals using a forked chain.
- [ ] Test every supported wallet.
- [ ] Test slow, rejected, replaced, and dropped transactions.

## Legal documents and customer disclosures

- [ ] Create a real Terms of Service page.
- [ ] Create a real Privacy Policy page.
- [ ] Fix footer links that currently point to absent routes.
- [ ] Add risk disclosures.
- [ ] Add custody disclosures.
- [ ] Add settlement-finality disclosures.
- [ ] Add fee disclosures.
- [ ] Add withdrawal timing and failure disclosures.
- [ ] Add token-specific risk disclosures.
- [ ] Add smart-contract risk disclosures.
- [ ] Add chain and bridge risk disclosures.
- [ ] Add operator-conflict disclosures.
- [ ] Add market-maker-conflict disclosures.
- [ ] Add regulatory-status disclosures.
- [ ] Add prohibited-jurisdiction disclosures.
- [ ] Add eligibility requirements.
- [ ] Add sanctions representations.
- [ ] Add customer-consent records.
- [ ] Add electronic-communications consent.
- [ ] Add dispute-resolution terms.
- [ ] Add complaint contact information.
- [ ] Add transaction receipt requirements.
- [ ] Add data-retention disclosures.
- [ ] Have counsel review every public claim concerning safety, custody, decentralization, and guarantees.

## Testing and launch gates

- [x] Preserve the currently passing Rust and Solidity suites; EX-021 passes the full Rust workspace, Clippy with warnings denied, Foundry formatting/build/tests, frontend validation, and the production Docker build.
- [ ] Add end-to-end custody tests across Rust, Solidity, RPC, and frontend layers.
- [ ] Test that every credited deposit has custody evidence.
- [ ] Test that no deposit event can be credited twice.
- [x] Test deposit reorganization handling, including provider disagreement, historical event invalidation, complete pending isolation, spent-funds quarantine, operation halts, retry idempotency, and audit-checkpoint rewind rejection.
- [ ] Test ledger and contract reconciliation.
- [ ] Test trade settlement across two users and two assets.
- [x] Test PostgreSQL withdrawal reservation and finalized completion, including exact balances, idempotent replay, and completion during a halt; deployed RPC and frontend end-to-end coverage remains open.
- [x] Test dropped and replaced withdrawal transactions at the PostgreSQL and worker boundaries, including immutable two-provider lineage, provider disagreement, wrong Ethereum nonce, replay, and terminal-attempt release safety; deployed RPC end-to-end testing remains open.
- [ ] Test emergency exit after trading.
- [ ] Test concurrent ordinary and emergency withdrawals.
- [ ] Test contract pause behavior.
- [ ] Test operator compromise assumptions.
- [ ] Test signer unavailability.
- [ ] Test storage loss.
- [ ] Test restart during every financial state transition.
- [ ] Test duplicate requests.
- [ ] Test delayed requests.
- [ ] Test messages arriving out of order.
- [ ] Test extreme-value and rounding behavior.
- [ ] Test fee conservation.
- [ ] Test token-decimal differences.
- [ ] Test malicious ERC-20 behavior.
- [ ] Test fee-on-transfer tokens.
- [ ] Test reentrant and reverting tokens.
- [ ] Add property tests asserting assets are always at least customer liabilities.
- [x] Add property tests asserting every journal transaction balances.
- [ ] Add property tests asserting users cannot withdraw more than finalized ownership.
- [ ] Run a capped adversarial testnet.
- [ ] Invite external traders and security researchers.
- [ ] Run a mainnet fork rehearsal.
- [ ] Conduct a full deploy-day rehearsal.
- [ ] Conduct a full incident-response rehearsal.
- [ ] Require a written go or no-go review before accepting real funds.

## Scope control and roadmap removals

- [ ] Pause work on perps.
- [ ] Pause work on borrow and lend.
- [ ] Pause work on portfolio margin.
- [ ] Pause work on the VLP and MM credit vault.
- [ ] Pause work on reputation-collateralized credit.
- [ ] Pause work on permissionless listings.
- [ ] Pause work on cross-chain bridge deposits.
- [ ] Pause work on AI-agent exchange features that do not improve custody safety.
- [ ] Pause work on additional exotic order types.
- [ ] Pause work on FIX expansion unless an institutional launch becomes the chosen strategy.
- [ ] Do not prioritize matcher microseconds over ledger correctness.
- [ ] Do not add more markets until two-asset settlement works reliably.
- [ ] Do not market synthetic liquidity as real liquidity.
- [ ] Do not accept significant funds during the transition.
- [ ] Do not deploy unaudited redesigned contracts.
- [ ] Do not use the existing settlement contract for a real-money launch.
- [ ] Do not treat passing component tests as proof of end-to-end solvency.

## Recommended phased launch

- [ ] Phase 0: Correct public claims and disable unsafe crediting.
- [ ] Phase 0: Define the legal entity, product model, and jurisdictions.
- [ ] Phase 0: Keep the system testnet-only.
- [ ] Phase 1: Implement the authoritative ledger and real deposit indexer.
- [ ] Phase 1: Implement atomic withdrawal accounting.
- [ ] Phase 1: Redesign settlement and emergency exits.
- [ ] Phase 1: Implement continuous reconciliation.
- [ ] Phase 2: Complete external security reviews.
- [ ] Phase 2: Complete legal and licensing preparation.
- [ ] Phase 2: Deploy a capped adversarial testnet.
- [ ] Phase 3: Launch ETH and USDC only with strict limits.
- [ ] Phase 3: Use invite-only access if permitted by the legal strategy.
- [ ] Phase 3: Monitor and reconcile continuously.
- [ ] Phase 3: Raise limits only after defined operational milestones.
- [ ] Phase 4: Add a small number of reviewed ERC-20 markets.
- [ ] Phase 4: Add compliant fiat or bridging partners if needed.
- [ ] Phase 5: Reconsider margin, lending, perps, vaults, and permissionless markets.

## Vercel and frontend deployment

- [x] Install the Vercel CLI with `npm i -g vercel` and authenticate the operator workstation.
- [ ] Use `vercel env pull` to manage and verify frontend environments.
- [ ] Use separate Vercel preview, staging, and production environments.
- [x] Validate that no private secret uses a `NEXT_PUBLIC_*` name in source, ignored local files, or deployed Vercel configuration.
- [x] Remove the public admin token configuration.
- [ ] Validate production chain IDs and contract addresses during builds.
- [ ] Add deployment-time checks rejecting Sepolia configuration in production.
- [ ] Use preview deployments for frontend review.
- [ ] Add browser verification to the deployment workflow.
- [ ] Use Vercel logs and observability for frontend failures.
- [ ] Keep the stateful matching engine and financial ledger on infrastructure designed for durable, long-running services.
- [ ] Do not move the engine into stateless frontend functions without redesigning its state model.
