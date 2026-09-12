# Fly engine health runbook

## Current diagnosis

Observed on 2026-09-12:

- DNS for `vela-engine.fly.dev` resolves and TLS terminates successfully at Fly.
- `GET /health` and `GET /status` send no response bytes and time out.
- The latest GitHub Actions deployment, run `33715787494` for commit `d600f303`, built successfully on 2026-09-03.
- During that deployment, Fly reported machine `2872d90f624448` reached the `stopped` state and then treated it as good.
- The `/health` handler returns a constant string and has no storage or network dependency. If the process is listening, it should respond immediately.

The confirmed fault domain is therefore Fly machine/process availability or startup, not DNS, TLS, or the health handler itself. The historical deployment workflow did not perform a public post-deploy smoke test, so a stopped or crash-looping machine could still produce a green workflow.

The repository-side resolution is:

1. Declare the deployed app explicitly as `testnet-demo` in `fly.toml`.
2. Require the deployment workflow to inspect machine/check state and receive a successful public `/health` response.
3. Keep this demo app separate from any future production app.

Live root-cause confirmation and recovery still require authenticated Fly control-plane access.

## Authenticated recovery

Run these commands from an operator workstation after `flyctl auth login`:

```sh
flyctl status --app vela-engine
flyctl machine list --app vela-engine
flyctl checks list --app vela-engine
flyctl logs --app vela-engine --no-tail
```

Identify why the app process stopped before mutating the machine. Common evidence includes a missing required secret, panic during snapshot/WAL recovery, out-of-memory termination, or an operator stop. Do not delete the volume or replace state as a recovery shortcut.

After correcting the identified cause, start or restart only the named app machine and verify both endpoints:

```sh
flyctl machine start 2872d90f624448 --app vela-engine
curl --fail --show-error --max-time 10 https://vela-engine.fly.dev/health
curl --fail --show-error --max-time 10 https://vela-engine.fly.dev/status
```

If the machine stops again, capture logs and leave it stopped until snapshot and WAL integrity are understood.

## Production requirement

Production must use separate liveness and readiness endpoints. Liveness must check only that the process/event loop can serve requests. Readiness must remain false until recovery completes and the ledger, custody RPC, indexer, signer, and reconciler are healthy. The current demo `/health` endpoint is not production readiness.
