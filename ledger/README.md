# Vela authoritative ledger

This crate is the PostgreSQL financial source of truth described by ADR-001. It owns append-only journal transactions, balanced journal legs, materialized account balances, idempotency, and the transactional outbox. The matching engine is not allowed to acknowledge financially effective work until this ledger commits.

## Local verification

The ledger and matcher integration tests create isolated databases through SQLx and therefore require a PostgreSQL administrator URL:

```sh
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p ledger -p api --test ledger_financial_commit
```

CI starts PostgreSQL 16 and runs the ledger tests as part of the workspace test suite. No production database is provisioned by this crate.

## Runtime configuration

The API uses `DATABASE_URL` only for the restricted application writer. It
creates a bounded pool, applies a statement timeout to every connection, and
retries serializable conflicts and deadlocks a bounded number of times. In
production, startup fails when `DATABASE_URL` is absent or the schema is not at
the exact version expected by the binary.

Migrations use a separate privileged secret and an explicit command:

```sh
DATABASE_MIGRATOR_URL=postgres://... cargo run --locked -p ledger --bin ledger-migrate
```

After migration, bootstrap the approved ETH/USDC asset definitions, platform accounts, and the custody-vault counter-accounts. The command is idempotent and rejects configuration drift:

```sh
DATABASE_MIGRATOR_URL=postgres://... \
VELA_CHAIN_ID=8453 \
VELA_USDC_ADDRESS=0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913 \
VELA_CUSTODY_VAULT_ADDRESS=0x... \
cargo run --locked -p ledger --bin ledger-bootstrap
```

The API never reads `DATABASE_MIGRATOR_URL` and never runs migrations at
startup. After the initial migration, apply `sql/roles.sql` as the database
owner. Create separate secret-bearing login roles outside the script and grant
each login exactly one of `vela_ledger_migrator`, `vela_ledger_writer`, or
`vela_ledger_reader`.

`GET /health` is process liveness. `GET /ready` checks database connectivity and
the exact ledger schema version whenever the ledger is configured. Production
returns not-ready when the ledger is absent or unhealthy; development and the
public testnet demo may run without a ledger. When configured, the matcher
keeps each batch private until its complete balanced financial effect commits;
failure discards the proposal and returns an internal error.

Ledger-backed API startup replays journal legs under a consistent database
snapshot, verifies the result against every materialized account balance, and
then replaces cached customer available/locked balances and platform fees. A
projection mismatch, malformed customer owner, overflow, or inexact decimal
conversion aborts startup.

Administrative balance corrections use `Ledger::post_administrative_correction`.
The typed command requires an incident, evidence reference, initiator, separate
approver, approval timestamp, and balanced legs. The complete request is hashed
for idempotency, and its journal, balance projection, outbox event, and immutable
audit record commit atomically. `Ledger::post` rejects the administrative
operation type so ordinary posting cannot bypass this audit boundary. Any future
operator API must authenticate both actor references before calling the command.

Credited-deposit contradictions use `Ledger::reverse_credited_deposit`. The
command verifies two-provider canonical evidence against the stored event,
activates an asset-wide halt, and atomically isolates available funds into the
customer pending account. A shortfall produces a quarantined deposit rather
than a false success. PostgreSQL blocks new credits, trading, and withdrawals
for an actively halted asset while allowing releases and compensating reversal.

Withdrawal policy limits are immutable, versioned database configuration owned
by the migrator role. Each asset requires an effective policy before
`Ledger::evaluate_withdrawal_policy` can approve a reservation. The evaluation
stores customer and platform velocity evidence plus a provider-neutral
compliance disposition. Missing policy, provider unavailability, review, active
asset halt, or active global halt fails closed before custody authorization.
The runtime writer may record policy decisions and activate the emergency halt,
but cannot publish policy versions or rewrite audit evidence. Resolving the
global halt requires three distinct operator identities.

See [the matcher-ledger commit boundary](../docs/architecture/matcher-ledger-commit.md)
for account requirements, decimal conversion, acknowledgement ordering, and
current limitations.

See [the ledger database runbook](../docs/operations/ledger-database.md) for
setup, migration, access, and recovery procedures.

See [the Base deposit indexer runbook](../docs/operations/base-deposit-indexer.md)
for immutable deployment registration, redundant RPC requirements, finalized
event ingestion, and halt response.

See [the withdrawal policy runbook](../docs/operations/withdrawal-policy.md)
for atomic-unit limit publication, provider disposition semantics, and global
halt handling.

Finalized withdrawal reverts and replacement lineage are immutable ledger
evidence. A replacement hash supplied by a reviewed relay or scanner must be
verified by two independent RPC providers against the latest attempt's sender,
Ethereum nonce, vault, and calldata hash before it can be registered. A revert
makes an attempt terminal but never releases reserved funds by itself; release
still requires post-deadline two-provider proof that the request and customer
nonce are unused, no execution event exists, and every attempt is terminal.

Continuous reconciliation is implemented as a separate, opt-in worker. At one
exact finalized block, two independent RPC providers must agree on chain ID,
block hash and timestamp, pinned vault runtime bytecode, native ETH balance,
and ERC20 `balanceOf(vault)` for every registered launch asset. In one
serializable ledger transaction, the worker persists immutable custody evidence
and calculates liability as the sum of every non-custody account projection.
That amount must independently equal the negated sum of the `custody_vault` and
`custody_withdrawn` projections. Missing projections, negative customer
balances, or disagreement between those derivations is indeterminate and
halts the asset. A custody deficit also halts. A surplus is retained as a
surplus, not called balanced, because it can be a finalized deposit that the
deposit indexer has not credited yet. Services never automatically unhalt.

See [the reconciliation worker runbook](../docs/operations/reconciliation-worker.md)
for failure semantics, startup requirements, and remaining orchestration limits.

## Migration rules

- Migrations in `migrations/` are append-only after they have reached a shared environment.
- Never edit or renumber an applied migration. Add a new forward migration.
- Destructive changes require an expand, migrate, verify, contract sequence.
- Journal and audit-history rows are corrected with new compensating records, never an update or delete.
- A production migration must be rehearsed on a restored backup and include an explicit rollback or forward-fix procedure.
