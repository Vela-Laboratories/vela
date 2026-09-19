# Vela V3 custody deployment runbook

**Target:** Base, after external audit and launch approval.
**Scope:** the single `VelaSettlement` omnibus custody vault.
**Status:** planning only; testnet-only until every outstanding gate is closed.

This runbook deliberately excludes the legacy Counter, Perp, VLP, and
Sequencer deployments. They are not part of the initial custody boundary.

## Pre-flight

- [ ] External smart-contract audit completed, findings fixed, and final report
      published.
- [ ] Audited source and backend frozen at a reviewed commit SHA.
- [ ] Foundry build, tests, formatting, and coverage evidence attached to the
      deployment ticket.
- [ ] Owner, guardian, and operator identities generated and reviewed under
      the approved custody policy; no signing keys in environment files.
- [ ] Canonical Base USDC address and chain ID verified independently.
- [ ] Ledger deposit event consumer, withdrawal reservation/signing path, and
      reconciliation worker reviewed together; managed hosting remains a gate.
- [ ] Legal, risk, customer disclosures, incident ownership, and live-funds
      approval completed.

## Reproducible artifact

```bash
cd contracts
./scripts/reproducible_build.sh
```

- [ ] Record the source commit, pinned compiler/EVM/optimizer configuration,
      creation-bytecode hash, and runtime-template hash.
- [ ] Confirm the deployment ticket contains the expected chain, owner,
      guardian, operator, and canonical USDC values.

## Deploy - VelaSettlement only

```bash
cd contracts
export RPC_URL=<approved-base-rpc>
export VELA_EXPECTED_CHAIN_ID=8453
export VELA_OPERATOR_ADDRESS=<reviewed-operator>
export VELA_OWNER_ADDRESS=<reviewed-owner>
export VELA_GUARDIAN_ADDRESS=<reviewed-guardian>
export VELA_USDC_ADDRESS=<canonical-base-usdc>
export VELA_SOURCE_COMMIT=0x<64-hex-byte-padded-git-commit>

forge script script/Deploy.s.sol:Deploy \
  --rpc-url "$RPC_URL" --broadcast --verify --slow -vvv
```

- [ ] Record the deployed address, transaction hash, chain ID, and final
      on-chain code hash in the deployment manifest.
- [ ] Verify `sourceCommit`, ETH support, USDC support, owner, guardian, and
      operator against the reviewed manifest.
- [ ] Verify source code and ABI on the block explorer.
- [ ] Confirm no Perp, VLP, Sequencer, or Counter contract was deployed by the
      initial custody procedure.

## Wiring and staged validation

- [ ] Configure the ledger/indexer with the deployed vault and consume the
      opaque-account `Deposited` event only after finality and receipt checks.
- [ ] Configure the withdrawal signer to produce V3 EIP-712 authorizations;
      relaying is permissionless, but ledger reservation and policy evidence
      remain mandatory.
- [ ] Exercise small-value ETH and USDC deposit and withdrawal flows on the
      approved environment, then reconcile custody against ledger liabilities.
- [ ] Configure independent deposit and withdrawal pause controls and rehearse
      owner resume plus guardian escalation.
- [ ] Verify managed hosting, RPC redundancy, monitoring, reconciliation, and
      rollback procedures before accepting any funds.

## Migration and incident response

The vault has no customer self-service emergency exit. If migration is needed:

1. Guardian or owner pauses both deposits and withdrawals.
2. Owner proposes a compatible successor and records the liability snapshot
   hash.
3. Guardian independently approves the proposal.
4. Wait the full seven-day migration timelock.
5. The operator signs the exact EIP-712 migration authorization, including the
   proposal, asset, amount, deadline, successor, and liability snapshot hash.
6. Owner executes the migration and independently verifies the received amount
   and emitted `AssetMigrated` evidence.
7. Reconcile the successor vault and ledger before resuming service.

For a suspected operator compromise, pause withdrawals, preserve evidence,
reconcile liabilities, rotate the operator through its delayed owner flow, and
obtain incident and audit approval before resuming. Do not broadcast an
unsigned or stale authorization.

## Post-deployment gates

- [ ] Hosted RPC and deployed end-to-end evidence recorded.
- [ ] Reconciliation and recovery worker running with alerts and on-call
      ownership.
- [ ] External audit, legal approval, managed hosting, and live-funds approval
      all recorded.
- [ ] Customer disclosures and production addresses published only after the
      preceding gates are complete.

*Last updated 2026-09-19 for the V3 custody-boundary redesign.*
