# Engine recovery integrity

The matching-engine snapshot and WAL are local recovery aids for order-book and other nonfinancial engine state. They are not the authoritative customer financial journal. When PostgreSQL is configured, customer balances and platform fee balances are rebuilt from the double-entry ledger after local engine recovery.

## Startup contract

Startup now fails before the listener is bound when any recovery input is ambiguous or invalid. This applies in every runtime mode.

- The configured `SNAPSHOT_DIR` must be readable and writable. There is no temporary-directory fallback.
- A fresh engine is allowed only when neither a snapshot nor prior WAL evidence exists.
- A snapshot without its WAL history, or WAL history without its snapshot, requires operator recovery.
- Snapshots must parse as the current version (`2`) and must have a self-consistent market/order layout and next-order sequence. The deployed version-1 format is accepted only at a strictly clean checkpoint/stop boundary, where its missing replay sequence can be derived without ambiguity; it is upgraded in memory and the next snapshot is written as version 2.
- Every retained WAL segment must be readable. Segment numbers and record sequences must be contiguous across the retained set, although the first retained sequence does not need to be `1`.
- Truncated headers, truncated payloads or checksums, oversized records, checksum mismatches, unsupported record types, duplicate sequences, regressions, gaps, and undecodable recovery entries stop startup.
- Only a valid terminal `ENGINE_STOP` record marks a clean shutdown. An older stop record followed by another entry is not clean.
- Automatic restart currently requires a clean snapshot, its immediately following checkpoint, and the terminal clean stop record. An unclean set is preserved and rejected until snapshot publication and request/WAL persistence share an atomic boundary.

The API does not accept traffic while these checks or replay are incomplete because the network listener is created only after recovery and the authoritative ledger replay succeed.

## Operator recovery procedure

1. Keep the engine stopped. Do not delete, truncate, rewrite, or renumber the snapshot or WAL segments in place.
2. Copy the complete snapshot and WAL directory to an immutable incident-evidence location. Record the deployed commit, runtime mode, volume identity, file metadata, and the exact startup error without recording credentials.
3. Determine whether the failure is storage loss, partial write, corruption, unsupported snapshot version, or an incomplete deployment migration. Preserve the original evidence throughout the investigation.
4. Verify an immutable bundle with `engine-recovery verify <bundle-dir> <trusted-verifying-key-file>`, then restore it with `engine-recovery restore <bundle-dir> <empty-snapshot-dir> <trusted-verifying-key-file>`. The target must reproduce the original snapshot path. Never combine a snapshot from one recovery set with WAL files from another.
5. Start one isolated engine instance and verify startup, local replay, authoritative ledger replay, schema readiness, and engine invariants before routing traffic.
6. Reconcile ledger liabilities against custody and review active asset halts before returning the instance to service. Recovery does not automatically clear a halt.

A deliberately new empty volume may be used only for a genuinely new environment with no prior state. It is not an incident-recovery technique.

## Remaining limitations

- Snapshot replacement, WAL checkpointing, and matcher publication are serialized by the mutation gate, while every acknowledged mutation is fsynced before publication. Automatic unclean takeover remains operator-gated because PostgreSQL and the local WAL are separate durability domains.
- Configured recovery bundles record SHA-256 and byte length for the snapshot and each WAL segment plus exact build identity and leadership generation. Exact manifest bytes now carry a detached signature verified against a separately pinned trust key; signing-key custody, rotation, revocation, and independent audit remain operational gates.
- WAL and local snapshots do not replace managed PostgreSQL replication, encrypted offsite backups, managed-provider point-in-time recovery, or a hosted lost-machine rehearsal.
- Recovery while deposits, orders, trades, or withdrawals are concurrently in flight has not been proven. Complete nonfinancial reconstruction still depends on the matched snapshot/WAL set.
- Runtime ledger, WAL, snapshot, leadership, and configured recovery-replication failures latch the process-lifetime matcher fence and stop mutating traffic. External paging is still not provisioned.
- An unclean version-1 snapshot cannot be migrated automatically because it does not record a replay boundary. Preserve the files and perform a reviewed operator recovery rather than inventing one.
