#!/bin/sh
set -eu

if [ ! -s "$PGDATA/PG_VERSION" ]; then
  echo "waiting for postgres-primary"
  until pg_isready -h postgres-primary -U "$POSTGRES_USER" -d "$POSTGRES_DB" >/dev/null 2>&1; do
    sleep 1
  done
  mkdir -p "$PGDATA"
  chown -R postgres:postgres "$PGDATA"
  gosu postgres pg_basebackup \
    -h postgres-primary \
    -U replicator \
    -D "$PGDATA" \
    -Fp -Xs -R -P -w
fi

exec docker-entrypoint.sh postgres
