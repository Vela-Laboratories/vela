# ADR-007: Recovery, replication, fencing, RPO, and RTO

- **Status:** Accepted
- **Date:** 2026-09-12

## Decision

The authoritative PostgreSQL ledger runs in a managed multi-availability-zone configuration with synchronous commit across failure domains, encrypted point-in-time recovery, daily backups, and regularly tested offsite export. Acknowledged financial operations have an RPO of zero. The service target is an RTO of 30 minutes for ledger and withdrawal recovery and 60 minutes for full trading recovery.

Exactly one ledger coordinator may commit matcher effects. Leadership uses a database-backed lease with monotonically increasing epochs. Every command, checkpoint, and outbox record carries the epoch; stale epochs are rejected at commit. Losing the lease immediately stops order acceptance. A replacement leader verifies reconciliation before reopening.

Engine checkpoints are versioned, checksummed, signed, and treated as acceleration artifacts. Recovery starts from the authoritative journal and verifies any checkpoint hash before replay. Corrupt, incompatible, or incomplete checkpoints fail closed; production never starts fresh or seeds state.

Restore, regional loss, split-brain, corrupt snapshot, partial write, and in-flight deposit/withdrawal scenarios are rehearsed quarterly. Backup success alone is not evidence of recoverability; measured restoration must meet the targets.

## Consequences

- A single local Fly volume and WAL cannot satisfy production durability.
- Failover is initially operator-approved active/passive, not automatic multi-writer.
- Client retries are safe through idempotency records retained with the journal.
