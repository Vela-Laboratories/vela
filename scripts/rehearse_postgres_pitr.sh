#!/usr/bin/env bash
set -euo pipefail

pitr_id="$(date +%s)-$$"
source_container="vela-pitr-source-${pitr_id}"
restore_container="vela-pitr-restore-${pitr_id}"
archive_volume="vela-pitr-archive-${pitr_id}"
backup_volume="vela-pitr-backup-${pitr_id}"

cleanup() {
  docker rm -f "$source_container" "$restore_container" >/dev/null 2>&1 || true
  docker volume rm "$archive_volume" "$backup_volume" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker volume create "$archive_volume" >/dev/null
docker volume create "$backup_volume" >/dev/null
docker run --rm --user root \
  -v "${archive_volume}:/archive" \
  -v "${backup_volume}:/basebackup" \
  postgres:16-alpine \
  sh -ec 'chown postgres:postgres /archive /basebackup'

docker run --name "$source_container" --detach \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -v "${archive_volume}:/archive" \
  -v "${backup_volume}:/basebackup" \
  postgres:16-alpine \
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

docker run --rm --user root -v "${backup_volume}:/basebackup" postgres:16-alpine \
  sh -ec 'touch /basebackup/recovery.signal && chown postgres:postgres /basebackup/recovery.signal'

docker run --name "$restore_container" --detach \
  -v "${archive_volume}:/archive:ro" \
  -v "${backup_volume}:/var/lib/postgresql/data" \
  postgres:16-alpine \
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
