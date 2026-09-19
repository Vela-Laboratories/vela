# M8 launch-qualification evidence package

**Status:** template and evidence index only; no launch approval is implied
**Operating state:** private, testnet-only, no real funds
**Custody boundary:** V3 `VelaSettlement` omnibus vault

This package is the source-controlled index for the M8 go/no-go review. Each
evidence record must identify the reviewed commit, environment, operator,
timestamp, inputs, outputs, and any limitation. Secrets, private keys, RPC
URLs, customer data, and unverifiable screenshots do not belong in Git.

## Evidence currently available locally

| Area | Evidence boundary | Result |
| --- | --- | --- |
| V3 custody contract | Foundry unit, invariant, pause, replay, migration, and fixed-vector tests | Local source evidence only; deployment and audit are open |
| Ledger | PostgreSQL transaction, idempotency, policy, halt, and execution-boundary tests | Local schema/service evidence only; hosted resilience is open |
| Deposits | Base indexer finality, two-provider, canonical-event, reorg, and halt logic | Local implementation evidence only; deployed V3 event evidence is open |
| Withdrawals | Reservation, V3 authorization binding, observer, finality, and expiry-release boundaries | Local/source-only evidence; RPC, broadcaster, signer, and deployed V3 evidence are open |
| Recovery | Local backup/WAL restore checks | Managed-provider replication, PITR, paging, and lost-machine rehearsal are open |

## Required M8 records

Create one completed record from the template for each rehearsal and external
review. A record marked `PASS` is not sufficient when a prerequisite is
unchecked or the environment is not the approved staging environment.

1. Capped adversarial testnet: deposits, trades, withdrawals, pause paths,
   replay, provider disagreement, reorganization, and insolvency/deficit halt.
2. Incident rehearsal: contract pause, signer compromise, RPC failure, database
   loss, reorganization, and insolvency response, including customer notice.
3. Mainnet-fork rehearsal and deploy-day rehearsal against the reviewed V3
   artifact manifest.
4. External smart-contract/settlement audit and API/infrastructure review,
   including finding remediation and signed release-commit identity.

## External gates intentionally unchecked

These are not satisfiable by local tests or this documentation change:

- external V3 smart-contract and settlement-system audit, remediation, and
  signed audited release;
- API and infrastructure penetration test, funded bug bounty, and independent
  security sign-off;
- deployed Base V3 testnet/mainnet-fork evidence, two independent RPCs,
  runtime-bytecode hash, event ABI, and reconciliation evidence;
- production signer/HSM or MPC, broadcaster, managed PostgreSQL replication,
  encrypted backups, PITR restore, paging, monitoring, and on-call ownership;
- legal approval, customer disclosures, supported jurisdictions/assets, market
  makers, market surveillance, and written go/no-go approval.

Until every applicable gate has a completed, independently reviewable record,
the repository remains testnet-only and no real funds may be accepted.
