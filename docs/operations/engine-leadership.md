# PostgreSQL engine leadership fencing

EX-032 prevents two ledger-backed matching-engine processes from accepting
matching mutations at the same time. Migration
`0012_engine_leadership.sql` creates one PostgreSQL lease row. Each acquisition
uses PostgreSQL server time and increments a monotonic generation. The owner
UUID is process-random and is never exposed by public status, readiness, logs,
or metrics.

## Safety boundary

The API acquires leadership before authoritative ledger replay, dispatcher
startup, or listener binding. Failure or contention aborts startup. The lease
renews every five seconds by default with a two-second timeout and a
fifteen-second lease. An expired, superseded, failed, or indeterminate renewal
latches the irreversible EX-031 durability fence. Liveness remains available,
but readiness and mutating traffic fail closed.

Every proposed matching batch, including resting orders and batches with no
net financial effect, extends and validates the exact owner and generation at
its commit boundary. Local WAL commit has a bounded timeout shorter than the
renewed lease. A financial batch additionally locks and validates the same
lease row inside the serializable PostgreSQL transaction that writes the
balanced journal, preventing a takeover from racing a stale financial commit.

`GET /ready` and `GET /status` expose only `configured`, `state`, `generation`,
and lease expiry. Metrics expose separate configured and active gauges. On
shutdown, renewal is disabled before releasing the exact owner and generation.
A failed release is safe: no other generation is cleared, and the server-time
lease expires naturally.

## Configuration

The defaults are:

- `VELA_ENGINE_LEASE_MS=15000`
- `VELA_ENGINE_LEASE_RENEW_MS=5000`
- `VELA_ENGINE_LEASE_TIMEOUT_MS=2000`

Operation timeout must be shorter than the lease, and renewal interval plus
operation timeout must also be strictly shorter than the lease. Operation
timeout controls acquisition, renewal, commit validation, release, and the
local WAL commit budget. Changes need a database-latency and filesystem-latency
review. Do not lengthen one value independently during an incident.

## Migration and activation

Do not grant the runtime application migration authority and do not run DDL at
API startup. Apply the migration with the privileged migrator identity:

```sh
DATABASE_MIGRATOR_URL=postgres://... \
cargo run --locked -p ledger --bin ledger-migrate
```

Then, as the database owner, reapply `ledger/sql/roles.sql`. Confirm schema
version 12, confirm the application writer can select and update only the
listed leadership columns, and confirm it cannot insert or delete the singleton
row. Start exactly one engine with its restricted `DATABASE_URL`. Verify:

1. logs report ledger connection and leadership acquisition without an owner ID;
2. `/ready` reports leadership `active` and generation greater than zero;
3. `vela_engine_leadership_configured` and
   `vela_engine_leadership_active` both equal one;
4. a second process refuses to bind while the first lease is active;
5. stopping the first process releases only its generation.

Do not force-expire or edit the row during ordinary operation. During suspected
split brain, remove both engines from traffic, preserve database and local WAL
evidence, and page the incident owner. Start a replacement only after the prior
lease is expired or explicitly released and the EX-030 recovery procedure has
established a safe local state.

## Current limitations

The live Fly testnet currently has no `DATABASE_URL`, so this feature is not
active there and a code deployment does not activate it. Development and demo
operation without PostgreSQL remains supported but has no multi-process safety.
Leadership does not replicate order books, WAL, or snapshots and does not make
automatic takeover safe. A new generation still requires an evidence-preserving
engine recovery procedure.

PostgreSQL and local WAL remain separate durability domains. A financial commit
followed by a WAL failure fences the process and requires operator recovery.
The timeout can cancel asynchronous lock waits, but an operating-system
filesystem call already executing may not be preemptible. External paging,
managed-database failover tests, long process-pause tests, and cross-failure-
domain engine-state replication remain launch gates.
