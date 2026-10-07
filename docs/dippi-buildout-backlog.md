# Dippi testnet buildout commit backlog

This is the ordered, reviewable implementation queue approved on 2026-10-07.
It refines the [consumer exchange engineering plan](consumer-exchange-engineering-plan.md)
without replacing its launch gates. A checked item means its code and focused
verification have landed, not that a hosted system or real-funds launch is ready.

The public site remains a holding page while this work proceeds. Staging uses
free services only. If a free staging option cannot satisfy a production
control, record the gap rather than treating the control as complete.

## Baseline

- [x] D-001 Reconcile the live buildout backlog with the launch plan.
- [x] D-002 Add a local PostgreSQL 16 and chain integration harness.
- [x] D-003 Validate compatible V3 testnet runtime configuration.
- [x] D-004 Test the public holding-page route boundary.

## Deposits, trading, and reconciliation

- [x] D-005 Validate V3 deployment manifests before registration.
- [ ] D-006 Verify distinct RPC providers and pinned custody code at startup.
- [ ] D-007 Prove an ETH deposit reaches the authoritative customer ledger.
- [ ] D-008 Prove a USDC deposit credits the exact received amount.
- [ ] D-009 Prove provider disagreement and reorganization containment.
- [ ] D-010 Prove a two-wallet trade transfers ownership and fees atomically.
- [ ] D-011 Run custody-to-liability reconciliation in the integrated flow.
- [ ] D-012 Prove a deficit halts affected financial operations.

## Testnet withdrawal lifecycle

- [ ] D-013 Add an opt-in testnet signer runtime.
- [ ] D-014 Verify signer chain, vault, code, and authorization preconditions.
- [ ] D-015 Persist the policy approval to signing handoff.
- [ ] D-016 Queue authorized transactions for controlled broadcast.
- [ ] D-017 Bound relayer nonce and fee decisions.
- [ ] D-018 Persist transaction submission and uncertain outcomes.
- [ ] D-019 Ingest trusted replacement candidates.
- [ ] D-020 Verify replacement lineage with two providers.
- [ ] D-021 Distinguish finalized revert, drop, and expiry outcomes.
- [ ] D-022 Journal remaining permanent failure and reversal outcomes.
- [ ] D-023 Prove ETH withdrawal through finality and reconciliation.
- [ ] D-024 Prove USDC withdrawal through finality and reconciliation.

## Staging and recovery

- [ ] D-025 Document free-only staging topology and production-control gaps.
- [ ] D-026 Verify hosted database roles, schema, and connection limits.
- [ ] D-027 Connect encrypted recovery export to an off-host target.
- [ ] D-028 Restore and verify a signed engine recovery bundle.
- [ ] D-029 Rehearse a named-point PostgreSQL restore.
- [ ] D-030 Rehearse recovery with financial operations in flight.
- [ ] D-031 Exercise operational alert delivery to a staging receiver.

## Customer flow

- [ ] D-032 Persist wallet sessions in a shared, revocation-capable store.
- [ ] D-033 Propagate session revocation across API replicas.
- [ ] D-034 Bound ingress authentication abuse.
- [ ] D-035 Persist scoped and expiring delegated trading permissions.
- [ ] D-036 Add emergency delegated-key revocation.
- [ ] D-037 Verify supported mobile wallet connection paths.
- [ ] D-038 Expose authoritative receipts and downloadable account history.
- [ ] D-039 Complete fee, limit, pending, and recovery guidance in the UI.

## Testnet qualification

- [ ] D-040 Fuzz custody and financial input boundaries.
- [ ] D-041 Add order price collars and order-value checks.
- [ ] D-042 Add observable market circuit breakers.
- [ ] D-043 Retain sequenced market-surveillance events.
- [ ] D-044 Rehearse load, reconnect, and infrastructure failures.
- [ ] D-045 Assemble reviewed M8 testnet evidence for exact commits.

## Evidence and release rules

- Financial-path commits include invariant-focused tests. Every commit keeps
  the applicable local checks passing and records behavior changes in docs.
- Deployed testnet evidence names the chain, deployment, commit, bytecode,
  provider identities, observed transaction hashes, and result without adding
  credentials or customer data to Git.
- A passing local test never closes an external gate. Managed production
  PostgreSQL resilience, a real production signer, external audits, legal
  approval, funded market makers, and the written go/no-go review remain open
  until separately evidenced in the M8 package.
- Mainnet deployment and real funds are outside this queue.

## Local integration dependencies

Run a command against fresh, loopback-only PostgreSQL 16 and Anvil instances:

```sh
./scripts/with_local_custody_deps.sh cargo test -p indexer --test pipeline
```

The harness applies ledger migrations, exports `DATABASE_URL`,
`DATABASE_MIGRATOR_URL`, and `ANVIL_RPC_URL` to its child command, and stops
both instances afterward. It requires local PostgreSQL 16, Foundry Anvil,
Python 3, and Cargo. Its Anvil keys are deterministic test fixtures and never
serve as hosted signer credentials.
