# Free-only Dippi testnet staging boundary

This is the current staging topology for buildout evidence. It uses local
PostgreSQL 16, Anvil, and repository checks without a paid hosted service or
real funds. The public site remains a holding page. No hosted V3 deployment,
managed database, independent RPC pair, or signer has been qualified by this
topology.

## Local evidence path

1. `scripts/with_local_custody_deps.sh` starts isolated loopback PostgreSQL
   and Anvil, applies the versioned ledger schema, runs one child command, and
   stops both processes.
2. `scripts/test_local_v3_deposits.sh` deploys V3 and a six-decimal token
   fixture, sends self-deposits from two local wallets, and advances Anvil's
   finalized block.
3. The opt-in integration test verifies deposit credit, matcher replay,
   customer-to-customer trade, fee and rebate journals, and custody-to-liability
   reconciliation. The deterministic provider simulations separately exercise
   disagreement, contradiction, and halt paths.
4. GitHub checks validate source, tests, formatting, dependency audits, and
   secrets. A passing check is evidence for the tested commit only.

Local Anvil's two loopback URL names address the same process. They test wiring
and fail-closed behavior, not independent provider failure. Deterministic Anvil
keys are fixtures, not deployable custody credentials.

## Hosted testnet admission

Before moving from local evidence to a free hosted testnet, record the chosen
services, free-tier limits, terms, operator, and shutdown path. Keep the API,
PostgreSQL, indexer, reconciliation worker, signer, and withdrawal observer in
separate roles. Use an isolated Base Sepolia V3 deployment and two genuinely
independent RPC origins. Register the exact reviewed deployment manifest,
bytecode, start block, and event ABI. Do not place a signing key in the web
process or repository. Exercise one capped testnet wallet flow and save
redacted transaction, journal, reconciliation, and recovery evidence for the
exact commit.

The existing Fly demo service is not this staging topology: its current
runtime and financial state are not proof of the V3 ledger path. A hosted
provider is not selected or provisioned by this document.

## Production-control gaps

Free-only local evidence does not satisfy production PostgreSQL replication,
point-in-time recovery, off-host encrypted backups and restore rehearsal,
independent RPC availability, protected signer/HSM or MPC, alert delivery,
external security review, incident staffing, legal approval, or the written
M8 go/no-go decision. Keep each gap open in the launch evidence package. If a
free hosted service cannot meet a required control, stop at testnet and record
the limitation rather than lowering the control.
