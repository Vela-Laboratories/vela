# Engine recovery replication and controlled failover

EX-033 and EX-034 add a provider-neutral replication boundary for nonfinancial matching-engine state. PostgreSQL remains authoritative for customer balances and platform financial accounts. The recovery bundle preserves the order book and other snapshot/WAL state needed to accelerate a reviewed active-passive recovery.

## Recovery bundle contract

Set `VELA_ENGINE_REPLICA_DIR` to a non-nested filesystem path and set `VELA_BUILD_ID` to the exact deployed Git commit. The process rejects a replica path that equals, contains, or is contained by `SNAPSHOT_DIR`. For production, mount that path from a separate failure domain. The process cannot prove that two sibling paths use different storage, so a local sibling path is useful only for development and does not satisfy the deployment gate.

Generate dedicated recovery keys into a new private directory:

```sh
engine-recovery generate-keys /secure/operator/recovery-keys
```

The command refuses an existing directory and writes the signing key and age identity with private file permissions. Back up the identity independently, with access controls and key-custodian approval. Configure the engine with:

- `VELA_RECOVERY_SIGNING_KEY_HEX`: contents of `recovery-signing-key.hex`, stored as a platform secret;
- `VELA_RECOVERY_VERIFYING_KEY_HEX`: contents of `recovery-verifying-key.hex`, pinned independently as the trust anchor;
- `VELA_ENGINE_OFFSITE_DIR`: non-nested destination for encrypted archives;
- `VELA_RECOVERY_AGE_RECIPIENT`: contents of `recovery-age-recipient.txt`.

Never place the age identity on the engine host. It is required only for an operator-controlled import.

Bundles created before EX-034 do not contain `recovery_signature.json` and are deliberately rejected by the current verifier. Preserve them as incident evidence, but create and independently verify a new signed bundle before relying on it for recovery. Never add a signature retroactively to make an old bundle appear production-approved.

At each periodic checkpoint, the engine holds the matcher mutation gate while it:

1. fsyncs and renames the snapshot;
2. appends and fsyncs the adjacent WAL checkpoint;
3. rotates the WAL;
4. copies the snapshot and retained WAL segments into a private temporary bundle;
5. records byte lengths and SHA-256 hashes with the snapshot version, build identity, leadership generation, and WAL boundary;
6. signs the exact manifest bytes with the dedicated secp256k1 recovery key and writes a detached signature containing the pinned public-key identity;
7. fsyncs the files and directory, atomically publishes the immutable bundle, and atomically advances `LATEST.json`;
8. streams the complete signed bundle into an authenticated age/X25519 archive and atomically publishes it to the configured offsite directory.

Configured replication, signing, and configured offsite export are one matching durability contract. Failure, timeout, or an indeterminate result latches the matcher-storage fence. `/health` remains live, while `/ready` and mutating traffic fail closed. Destinations and key material are never included in public status or metrics.

When replication is configured, startup publishes and verifies one complete periodic recovery bundle before binding the network listener. A process cannot begin accepting traffic with a configured but unproven replica target.

Periodic bundles are deliberately marked unclean. They preserve a precise investigation and recovery point, but they are not eligible for unattended takeover. During graceful shutdown, the periodic task is stopped before the final snapshot. The engine then writes the final checkpoint, flushes the exact terminal `ENGINE_STOP`, and publishes a clean bundle.

## Verification and restore

Keep both engines out of traffic and preserve the failed host or volume as incident evidence. Verify a selected immutable bundle before restoration:

```sh
engine-recovery verify-latest /replica /secure/trust/recovery-verifying-key.hex
engine-recovery verify \
  /replica/checkpoint-00000000000000000123-abcd1234abcd1234 \
  /secure/trust/recovery-verifying-key.hex
```

The verifier rejects an untrusted signing key, a changed signature or manifest, unexpected files in the bundle, path traversal, symlinks, duplicate artifacts, wrong byte lengths or SHA-256 hashes, malformed snapshots, WAL corruption or sequence gaps, mismatched checkpoint paths, and false clean-shutdown claims.

An operator can also export an existing signed bundle or import an encrypted archive without placing secrets on a command line:

```sh
engine-recovery export \
  /replica/checkpoint-00000000000000000123-abcd1234abcd1234 \
  /offsite \
  /secure/trust/recovery-age-recipient.txt \
  /secure/trust/recovery-verifying-key.hex

engine-recovery import \
  /offsite/checkpoint-00000000000000000123-abcd1234abcd1234.age \
  /replacement-replica \
  /secure/operator/recovery-age-identity.txt \
  /secure/trust/recovery-verifying-key.hex
```

Import decrypts into a private staging directory with bounded entry names, counts, and sizes. It accepts the bundle only after age authentication, exact archive framing, and trusted signature and recovery-boundary verification all pass.

Restore only into an absent or empty replacement directory mounted at the same logical path as the source `SNAPSHOT_DIR`:

```sh
engine-recovery restore \
  /replica/checkpoint-00000000000000000123-abcd1234abcd1234 \
  /data \
  /secure/trust/recovery-verifying-key.hex
```

The command verifies first and refuses unclean bundles. It copies a clean bundle into a private sibling staging directory, fsyncs it, and atomically renames it into place. It never merges with existing state and refuses a nonempty target.

For a controlled active-passive failover:

1. Stop order routing and verify the old leader is no longer accepting mutations.
2. Gracefully stop the old leader when possible and verify a new clean bundle appears.
3. Verify the bundle and record its build ID, leadership generation, checkpoint, hashes, and source host in the incident record.
4. Restore onto an empty replacement volume at the original snapshot path.
5. Start exactly one isolated replacement with the same compatible binary and authoritative PostgreSQL ledger.
6. Verify local recovery, ledger replay, engine leadership acquisition, reconciliation results, asset halts, and readiness before restoring traffic.
7. Preserve both the original evidence and selected bundle until post-incident review is complete.

Never force-edit a manifest, snapshot, WAL segment, `LATEST.json`, or leadership row to make verification pass.

## Deployment gates still open

The live Fly testnet does not configure `DATABASE_URL` or `VELA_ENGINE_REPLICA_DIR`; pushing this code does not activate authoritative ledger, leadership, or recovery replication there. Before real funds, provision and rehearse all of the following:

- a replica and encrypted archive target in a genuinely different failure domain with tested access controls, retention, and immutable/versioned storage;
- managed PostgreSQL synchronous multi-zone replication, encrypted offsite backups, and point-in-time recovery;
- alerts for replication lag/failure, backup failure, database saturation, and readiness loss;
- a measured lost-machine, regional-loss, PostgreSQL PITR, and in-flight deposit/withdrawal recovery exercise;
- independent custody, rotation, revocation, and disaster recovery for the recovery signing and age identity keys;
- an operator-approved procedure for reconciling PostgreSQL commits that lack a corresponding local WAL mutation.

The code provides signed artifacts, authenticated encrypted export/import, and a fail-closed restore boundary. A local directory adapter does not itself prove offsite or cross-region durability, provider retention, RPO zero, the 30/60-minute RTO targets, or safe automatic takeover.

CI separately runs `scripts/rehearse_postgres_pitr.sh` to restore PostgreSQL 16 from a physical base backup and archived WAL to an exact named point. That is repository-level PITR evidence only; the managed-provider rehearsal above remains mandatory.
