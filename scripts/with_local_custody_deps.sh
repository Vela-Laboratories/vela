#!/usr/bin/env bash
set -euo pipefail

if (( $# == 0 )); then
  printf 'usage: %s command [args...]\n' "$0" >&2
  exit 2
fi

for required in initdb pg_ctl pg_isready createdb psql postgres anvil python3 cargo; do
  if ! command -v "$required" >/dev/null 2>&1; then
    printf 'missing local dependency: %s\n' "$required" >&2
    exit 2
  fi
done

postgres_major="$(postgres --version | awk '{print $3}' | cut -d. -f1)"
if [[ "$postgres_major" != 16 ]]; then
  printf 'PostgreSQL 16 is required; found major version %s\n' "$postgres_major" >&2
  exit 2
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/dippi-custody-XXXXXX")"
pg_data="$work_dir/postgres"
anvil_pid=''
postgres_running=false

cleanup() {
  status=$?
  trap - EXIT
  if [[ -n "$anvil_pid" ]]; then
    kill "$anvil_pid" >/dev/null 2>&1 || true
    wait "$anvil_pid" >/dev/null 2>&1 || true
  fi
  if [[ "$postgres_running" == true ]]; then
    pg_ctl -D "$pg_data" -m immediate stop >/dev/null 2>&1 || true
  fi
  rm -rf -- "$work_dir"
  exit "$status"
}
trap cleanup EXIT

# Both listeners bind loopback only. This probe reserves no port, so the
# subsequent readiness checks also catch an intervening local process.
read -r pg_port anvil_port < <(python3 - <<'PY'
import socket

sockets = []
try:
    for _ in range(2):
        sock = socket.socket()
        sock.bind(('127.0.0.1', 0))
        sockets.append(sock)
    print(*(sock.getsockname()[1] for sock in sockets))
finally:
    for sock in sockets:
        sock.close()
PY
)

initdb -D "$pg_data" -A trust -U postgres --no-instructions >"$work_dir/initdb.log" 2>&1
pg_ctl -D "$pg_data" -l "$work_dir/postgres.log" \
  -o "-h 127.0.0.1 -p $pg_port -k $work_dir" start >/dev/null
postgres_running=true
pg_isready -h 127.0.0.1 -p "$pg_port" -U postgres >/dev/null
createdb -h 127.0.0.1 -p "$pg_port" -U postgres vela_test

anvil --host 127.0.0.1 --port "$anvil_port" --chain-id 84532 \
  >"$work_dir/anvil.log" 2>&1 &
anvil_pid=$!

export ANVIL_RPC_URL="http://127.0.0.1:$anvil_port"
export DATABASE_URL="postgres://postgres@127.0.0.1:$pg_port/vela_test"
export DATABASE_MIGRATOR_URL="$DATABASE_URL"

ready=false
for _ in $(seq 1 50); do
  if ANVIL_RPC_URL="$ANVIL_RPC_URL" python3 - <<'PY' >/dev/null 2>&1
import json
import os
import urllib.request

request = urllib.request.Request(
    os.environ['ANVIL_RPC_URL'],
    data=json.dumps({'jsonrpc': '2.0', 'method': 'eth_chainId', 'params': [], 'id': 1}).encode(),
    headers={'Content-Type': 'application/json'},
)
with urllib.request.urlopen(request, timeout=1) as response:
    assert int(json.load(response)['result'], 16) == 84532
PY
  then
    ready=true
    break
  fi
  sleep 0.1
done
if [[ "$ready" != true ]]; then
  printf 'local chain did not become ready\n' >&2
  exit 1
fi

cargo run --quiet --locked -p ledger --bin ledger-migrate
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -At \
  -c 'SELECT max(version) FROM _sqlx_migrations WHERE success' \
  | grep -qx '17'

"$@"
