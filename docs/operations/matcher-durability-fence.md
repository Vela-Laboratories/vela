# Matcher durability fence

EX-031 moves local mutation persistence into the matching engine's private
proposal lifecycle. A post, cancel, demo deposit, or demo withdrawal is not
published to the committed sharded state and its caller is not sent a success
response until its complete request/response batch has been appended to the
local WAL and `fsync` has succeeded.

The dispatcher and snapshot publisher share one process-wide mutation gate.
Snapshots are assembled from the committed `MarketShards` and `UserState`, not
from the bootstrap engine copy. The snapshot file and directory are synced,
then the matching WAL checkpoint is appended, flushed, and rotated while the
gate remains held.

## Failure behavior

Any financial-effect validation failure, authoritative-ledger commit failure,
mutation-WAL append or flush failure, snapshot publication failure, checkpoint
failure, or WAL rotation failure latches the matcher storage fence for the
remainder of the process lifetime.
The fence cannot be cleared through an API.

Once fenced:

- the proposal that observed the failure is discarded from in-memory matching
  state and receives an internal error;
- requests already queued behind that proposal cannot publish state;
- every new non-GET HTTP request receives `503 Service Unavailable`;
- `GET /health` remains `200` because the process is alive;
- `GET /ready` returns `503` with a stable, non-sensitive failure code;
- `GET /status` reports `fenced` and the same matcher-storage summary;
- `/metrics` exposes `vela_matcher_storage_healthy 0` and increments
  `vela_matcher_storage_failures_total`;
- periodic and shutdown snapshots are refused, so shutdown cannot create a
  clean recovery boundary after a durability failure.

The operator should remove the instance from service, preserve the snapshot,
all WAL segments, and PostgreSQL evidence, and follow the evidence-preserving
procedure in [engine recovery integrity](engine-recovery-integrity.md). Do not
restart by deleting or editing local evidence.

`MUTATION_BATCH` is a forward WAL format change. After this release writes its
first mutation batch, do not roll the same volume back to a pre-EX-031 binary,
whose WAL reader does not recognize that record type. A rollback requires an
explicit compatible reader or an operator-reviewed recovery onto a separate
volume.

## Cross-resource boundary

PostgreSQL remains authoritative for customer balances and platform fees. For
a financially effective matching batch, PostgreSQL commits first and the local
WAL commits second, before matching state publication. This is a publication
gate, not a distributed transaction. A crash or WAL failure after PostgreSQL
commits can leave a durable financial effect without a published local order
book mutation. The process therefore fences and leaves an unclean recovery set;
automatic unclean replay remains disabled and operator reconciliation is
required.

## Remaining limits

- There is no two-phase commit across PostgreSQL and the local matching WAL.
- Snapshot replacement and WAL checkpointing are serialized and fail closed,
  but do not form one filesystem transaction. A crash between them still
  requires operator recovery.
- Local WAL and snapshots are not replicated across failure domains.
- Automatic unclean replay remains disabled.
- Directly managed auxiliary product registries are not yet part of the
  durable matching mutation batch.
- Prometheus signals are present, but no external alert receiver or pager is
  provisioned by this chapter.
- Leader election and fencing between two engine processes remain open.
- Full lost-machine, point-in-time, and in-flight chain-operation recovery
  rehearsals remain open.
