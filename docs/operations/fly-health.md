# Fly engine health runbook

## Incident diagnosis and resolution

Observed and repaired on 2026-09-12:

- DNS for `vela-engine.fly.dev` resolved and TLS terminated successfully at Fly, but `GET /health` and `GET /status` timed out because the only machine was stopped.
- The latest GitHub Actions deployment, run `33715787494` for commit `d600f303`, built successfully on 2026-09-03.
- During that deployment, Fly reported machine `2872d90f624448` reached the `stopped` state and then treated it as good.
- Authenticated logs showed three startup failures: the service could not write its WAL because `/data` contents were owned by `root`, the existing snapshot was incompatible with the running binary, and the required `ADMIN_TOKEN` secret was absent.
- Fly stopped restarting the machine after it reached the maximum restart count.

The incident was recovered without deleting the volume:

1. The machine was placed in a temporary maintenance command while the volume was inspected.
2. `/data` ownership was corrected to service UID and GID `10001`.
3. The incompatible snapshot was preserved as `/data/engine_snapshot.incompatible-20260912.json`.
4. A new strong `ADMIN_TOKEN` Fly secret was generated and installed without writing it to the repository or printing it.
5. The normal `api` command was restored and the machine was restarted.

After recovery, Fly reported one passing health check, public `/health` returned `ok`, and `/status` reported `operational`. The service also created fresh snapshot and WAL files owned by UID and GID `10001`. The recovered demo state is newly initialized; the incompatible historical snapshot and Fly volume snapshots remain available for forensic analysis or a purpose-built migration.

The root cause was therefore startup configuration and persistent-volume compatibility, not DNS, TLS, or the health handler. The historical deployment workflow did not perform a public post-deploy smoke test, so a stopped or crash-looping machine could still produce a green workflow.

The repository-side resolution is:

1. Declare the deployed app explicitly as `testnet-demo` in `fly.toml`.
2. Require the deployment workflow to inspect machine/check state and receive a successful public `/health` response.
3. Keep this demo app separate from any future production app.

## Authenticated recovery

For a future incident, run these commands from an operator workstation after `flyctl auth login`:

```sh
flyctl status --app vela-engine
flyctl machine list --app vela-engine
flyctl checks list --app vela-engine
flyctl logs --app vela-engine --no-tail
```

Identify why the app process stopped before mutating the machine. Common evidence includes a missing required secret, panic during snapshot/WAL recovery, out-of-memory termination, or an operator stop. Do not delete the volume or replace state as a recovery shortcut. Preserve incompatible state under a dated name and confirm a recoverable Fly volume snapshot exists before attempting migration or repair.

After correcting the identified cause, start or restart only the named app machine and verify both endpoints:

```sh
flyctl machine start 2872d90f624448 --app vela-engine
curl --fail --show-error --max-time 10 https://vela-engine.fly.dev/health
curl --fail --show-error --max-time 10 https://vela-engine.fly.dev/status
```

If the machine stops again, capture logs and leave it stopped until snapshot and WAL integrity are understood.

## Production requirement

Production must use separate liveness and readiness endpoints. Liveness must check only that the process/event loop can serve requests. Readiness must remain false until recovery completes and the ledger, custody RPC, indexer, signer, and reconciler are healthy. The current demo `/health` endpoint is not production readiness.
