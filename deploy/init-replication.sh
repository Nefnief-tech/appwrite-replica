#!/bin/bash
set -e

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
  create role replicator with replication login password '$REPLICATION_PASSWORD';
EOSQL

# Both the fixed replicas (replicator) and console-spawned replicas
# (leader DSN user) need replication access from the Docker network.
echo "host replication all all scram-sha-256" >> "$PGDATA/pg_hba.conf"
