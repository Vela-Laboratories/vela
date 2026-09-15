# Continuous custody reconciliation

EX-029 adds an opt-in, testnet-only `reconciliation-worker`. It is not started
by the API image and no hosted RPC, database credential, or deployment is
configured by this chapter.

## Safety boundary

Each cycle acquires a PostgreSQL lease and reads one exact finalized block from
two distinct HTTPS RPC origins. Both providers must return the registered chain
ID and identical block number, hash, and timestamp. The worker then verifies
the vault runtime bytecode against the immutable deployment registry and reads
native ETH or ERC20 `balanceOf(vault)` for every registered chain asset at that
same block. Provider disagreement, stale or malformed evidence, wrong chain,
or wrong bytecode is preserved as immutable failure evidence and activates a
full asset halt before retry.

The ledger transaction uses serializable isolation. Liability is:

`sum(all customer and platform account balance projections)`

The independent accounting cross-check is:

`liability = -sum(custody_vault + custody_withdrawn projections)`

Missing balance rows, negative customer balances, a negative aggregate, or a
cross-check mismatch is `indeterminate`. Custody below a valid liability is a
`deficit`. Both statuses atomically create or reuse an active halt covering
deposit credits, trading, and new withdrawals. A surplus is stored explicitly
and does not claim exact reconciliation: finalized custody can temporarily be
ahead of deposit crediting. Conversely, an unjournaled finalized outbound move
can appear as a deficit and must halt even if a withdrawal observer later
accounts for it. A balanced result never resolves an existing halt.

## Configuration

The worker fails closed unless all required values are set:

- `VELA_RECONCILIATION_ENABLED=true`
- `DATABASE_URL` for the restricted ledger writer
- `VELA_RECONCILIATION_DEPLOYMENT_ID`
- `VELA_RECONCILIATION_RPC_PRIMARY_NAME`
- `VELA_RECONCILIATION_RPC_PRIMARY_URL`
- `VELA_RECONCILIATION_RPC_SECONDARY_NAME`
- `VELA_RECONCILIATION_RPC_SECONDARY_URL`

Optional bounded controls are `VELA_RECONCILIATION_RPC_TIMEOUT_MS`,
`VELA_RECONCILIATION_POLL_MS`, `VELA_RECONCILIATION_RETRY_MAX_MS`,
`VELA_RECONCILIATION_LEASE_SECONDS`, and
`VELA_RECONCILIATION_MAX_FINALIZED_AGE_SECONDS`. URLs may use HTTP only for
loopback tests and may not contain userinfo. Logs and errors identify providers
by configured name and do not print RPC URLs or response bodies.

Run migrations and apply `ledger/sql/roles.sql` with the privileged migration
identity before starting the worker. Graceful shutdown releases its lease;
crash recovery safely reacquires an expired lease and deterministic evidence
hashes make a repeated finalized observation idempotent.

Migration `0011_continuous_reconciliation.sql` deliberately aborts if legacy
custody snapshots or reconciliation results already exist because those rows
lack the required deployment and dual-provider provenance. Export and review
such rows before migration; do not delete or silently reinterpret them merely
to make the migration pass.

## Readiness and recovery limits

The checkpoint and immutable result history provide the data needed for a
future readiness surface, but EX-029 does not add API readiness wiring, external
paging, daily signed reports, or automated halt resolution. A processor-aware
orchestrator must eventually verify that deposit and withdrawal checkpoints
have caught up to the custody snapshot before treating `balanced` as a complete
system-wide readiness claim. Operators must retain incident evidence, perform
a fresh reconciliation after recovery, and use the existing dual-control halt
resolution workflow. Never infer production solvency from this source-only,
unconfigured testnet worker.
