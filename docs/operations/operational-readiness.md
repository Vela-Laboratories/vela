# Operational readiness and alert delivery

`/health` is process liveness. `/ready` is the fail-closed traffic gate. In
production, readiness now requires the authoritative ledger plus successful
bounded probes of the chain indexer, custody signer, custody RPC adapter, and
reconciliation worker.

Configure each dependency with a private readiness endpoint:

- `VELA_INDEXER_READINESS_URL`
- `VELA_SIGNER_READINESS_URL`
- `VELA_CUSTODY_RPC_READINESS_URL`
- `VELA_RECONCILER_READINESS_URL`

`VELA_DEPENDENCY_TIMEOUT_MS` defaults to 2000 and is bounded between 100 and
30000. `VELA_DEPENDENCY_POLL_SECONDS` defaults to 15 and is bounded between 5
and 300. Any timeout, request failure, or non-success response makes the
dependency unhealthy. Production startup rejects missing dependency endpoints.
Development and the testnet demo may leave them unconfigured without claiming
production readiness.

## Alerts

Set `VELA_ALERT_WEBHOOK_URL` to a separately operated incident receiver. The
background monitor sends `vela.operational-alert.v1` JSON only when the
redacted health fingerprint changes. It covers matcher storage/WAL/snapshot
fences, recovery-replication health, and the four required dependency probes.
The webhook URL is never returned by readiness, status, metrics, or alert
payloads. Delivery has a bounded timeout, failures are logged, and a failed
delivery is retried on the next poll because its fingerprint is not recorded as
delivered.

Prometheus exports `vela_operational_dependency_healthy{dependency="..."}`.
The readiness response exposes only configured/healthy/failure-code fields.

This adapter does not create an on-call rotation or independently hosted
receiver. Before accepting real funds, route it to a tested paging destination,
define severities and escalation ownership, and prove delivery during a hosted
failure exercise.
