#!/usr/bin/env bash
set -euo pipefail

pitr_id="$(date +%s)-$$"
postgres_image="${POSTGRES_IMAGE:-postgres:16-alpine@sha256:cf78e76683b9ca8c5733cbbdce6c9262b45b6767934dd0a95e671f9a0fc20685}"
failure_line="unknown"
source_container="vela-pitr-source-${pitr_id}"
restore_container="vela-pitr-restore-${pitr_id}"
archive_volume="vela-pitr-archive-${pitr_id}"
backup_volume="vela-pitr-backup-${pitr_id}"

cleanup() {
  docker rm -f "$source_container" "$restore_container" >/dev/null 2>&1 || true
  docker volume rm "$archive_volume" "$backup_volume" >/dev/null 2>&1 || true
}

finish() {
  status=$?
  trap - EXIT
  if (( status != 0 )); then
    printf 'PITR rehearsal failed at line %s with status %s\n' "$failure_line" "$status" >&2
    for container in "$source_container" "$restore_container"; do
      if docker inspect "$container" >/dev/null 2>&1; then
        printf '%s\n' "--- ${container} state ---" >&2
        docker inspect "$container" --format '{{json .State}}' >&2 || true
        printf '%s\n' "--- ${container} logs ---" >&2
        docker logs "$container" >&2 || true
      fi
    done
  fi
  cleanup
  exit "$status"
}
trap finish EXIT
trap 'failure_line=$LINENO' ERR

printf 'Pulling pinned PostgreSQL 16 image %s\n' "$postgres_image"
docker pull "$postgres_image" >/dev/null
docker volume create "$archive_volume" >/dev/null
docker volume create "$backup_volume" >/dev/null
prepared=false
for attempt in 1 2 3; do
  if docker run --rm --user root \
    -v "${archive_volume}:/archive" \
    -v "${backup_volume}:/basebackup" \
    "$postgres_image" \
    sh -ec 'chown postgres:postgres /archive /basebackup'; then
    prepared=true
    break
  fi
  printf 'Volume preparation attempt %s failed; retrying\n' "$attempt" >&2
  sleep "$attempt"
done
if [[ "$prepared" != true ]]; then
  printf 'Unable to prepare PostgreSQL backup volumes\n' >&2
  exit 1
fi

printf 'Starting PostgreSQL source cluster\n'
docker run --name "$source_container" --detach \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -v "${archive_volume}:/archive" \
  -v "${backup_volume}:/basebackup" \
  "$postgres_image" \
  -c wal_level=replica \
  -c archive_mode=on \
  -c 'archive_command=cp %p /archive/%f' >/dev/null

for _ in $(seq 1 60); do
  if docker exec "$source_container" pg_isready -U postgres >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
docker exec "$source_container" pg_isready -U postgres >/dev/null

psql_source() {
  docker exec "$source_container" psql -v ON_ERROR_STOP=1 -U postgres -d postgres "$@"
}

psql_source -c 'CREATE TABLE pitr_probe (id BIGINT PRIMARY KEY, marker TEXT NOT NULL);' >/dev/null
psql_source -c "INSERT INTO pitr_probe VALUES (1, 'before-target');" >/dev/null

docker exec "$source_container" \
  pg_basebackup -h 127.0.0.1 -U postgres -D /basebackup -Fp -Xs >/dev/null

psql_source -c "SELECT pg_create_restore_point('vela_recovery_target');" >/dev/null
psql_source -c "INSERT INTO pitr_probe VALUES (2, 'after-target');" >/dev/null
psql_source -c 'SELECT pg_switch_wal(); CHECKPOINT; SELECT pg_switch_wal();' >/dev/null
docker stop --time 30 "$source_container" >/dev/null

docker run --rm --user root -v "${backup_volume}:/basebackup" "$postgres_image" \
  sh -ec 'touch /basebackup/recovery.signal && chown postgres:postgres /basebackup/recovery.signal'

printf 'Starting PostgreSQL point-in-time restore\n'
docker run --name "$restore_container" --detach \
  -v "${archive_volume}:/archive:ro" \
  -v "${backup_volume}:/var/lib/postgresql/data" \
  "$postgres_image" \
  -c 'restore_command=cp /archive/%f %p' \
  -c 'recovery_target_name=vela_recovery_target' \
  -c 'recovery_target_action=promote' >/dev/null

for _ in $(seq 1 60); do
  if docker exec "$restore_container" pg_isready -U postgres >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
docker exec "$restore_container" pg_isready -U postgres >/dev/null

restored_rows="$(docker exec "$restore_container" \
  psql -At -v ON_ERROR_STOP=1 -U postgres -d postgres \
  -c "SELECT string_agg(id::text || ':' || marker, ',' ORDER BY id) FROM pitr_probe;")"

if [[ "$restored_rows" != "1:before-target" ]]; then
  printf 'PITR verification failed: expected only the pre-target row, got %s\n' "$restored_rows" >&2
  exit 1
fi

printf 'PostgreSQL 16 PITR rehearsal passed: restored exactly 1:before-target\n'
