# Deploying with Dokploy

The repository ships a Dokploy-specific Compose file: `compose.dokploy.yml`. It runs the whole
stack (Postgres leader + two replicas, Redis, API, web) as **one Compose service** on the external
`dokploy-network`, with no host ports. Dokploy's Traefik and Domains tab handle public access.

## Deploy

1. In Dokploy, create a Project, then **Create Service → Compose**.
2. Set the provider to your Git provider, choose this repository and branch, and set
   **Compose Path** to `compose.dokploy.yml` (or paste the file with the Raw provider).
3. In the service's **Environment** tab, add the variables below, Save, then Deploy.
4. Once healthy, open the **Domains** tab, add a domain for the `web` service on port **3000**,
   enable HTTPS (Let's Encrypt), and point `PUBLIC_URL` at that `https://` URL.
5. Sign in at the domain with `ADMIN_EMAIL` / `ADMIN_PASSWORD`.

Only the `web` service needs a domain. It proxies `/admin/api` and `/v1` to `api:8080` internally.

## Environment variables

| Variable | Required | Default / note |
|---|---|---|
| `POSTGRES_PASSWORD` | yes | database password |
| `REPLICATION_PASSWORD` | yes | used by the two fixed replicas |
| `SESSION_SECRET` | yes | ≥ 24 chars |
| `NODE_SECRET` | yes | encrypts stored node DSNs; changing it invalidates them |
| `ADMIN_PASSWORD` | yes | change from the development default |
| `PUBLIC_URL` | yes | your `https://` domain |
| `POSTGRES_USER` | no | `openwrite` |
| `POSTGRES_DB` | no | `openwrite` |
| `ADMIN_EMAIL` | no | `admin@openwrite.local` |
| `COOKIE_SECURE` | no | `true` (keep it with HTTPS) |
| `COOKIE_SAMESITE` | no | `lax` |
| `CORS_ORIGINS` | no | comma-separated; empty is fine for same-origin use |
| `READ_ROUTING` | no | `replica` |
| `DOCKER_NETWORK` | no | `dokploy-network` — must match network of spawned replicas |

`init-replication.sh` allows replication from any role with the replication attribute, so both
the fixed replicas (`replicator`) and console-spawned replicas (leader DSN user) work. On a
database initialised by an older version, add
`host replication all all scram-sha-256` to `pg_hba.conf` and reload Postgres.

## Docker socket and replica spawning

The `api` service mounts `/var/run/docker.sock` so **Nodes → Spawn replica** can create Postgres
containers on the Dokploy host. Spawned replicas join `DOCKER_NETWORK` and are reachable by
container name. **Docker socket access is host-root-equivalent**: anyone with admin console access
can manage containers on the host. Keep the admin account and domain access restricted.

If you do not want that capability, remove the socket mount from `compose.dokploy.yml` and use
the manual **Register node** flow instead; `/admin/api/docker/status` will report Docker as
unavailable and the UI will disable the button.

## Volumes and upgrades

Named volumes `pg-primary`, `pg-replica-1`, `pg-replica-2`, `redis-data` and `api-data` hold all
state. Back them up before redeploying or changing `NODE_SECRET`. Dokploy's scheduled Postgres
backup can target the `postgres-primary` service. To upgrade, push the new commit and press
Redeploy; the Compose file is read from Git, so keep it in the repository.
