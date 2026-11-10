# Openwrite

A self-hosted, Appwrite-compatible backend on PostgreSQL and Redis, with a modern admin console.

Openwrite keeps the Appwrite v1 API surface your SDKs already speak, but stores documents in
ordinary PostgreSQL tables — no proprietary document engine, no opaque migrations. One primary
takes writes, any number of streaming replicas can serve reads, and projects can be imported
from Appwrite Cloud or self-hosted Appwrite.

- **API** — Rust (Axum, SQLx), Appwrite-compatible `/v1` routes
- **Storage** — PostgreSQL; one schema per logical database, one table per collection
- **Sessions** — Redis-backed httpOnly cookies, Argon2 password hashes
- **Console** — Next.js admin UI for projects, users, data, nodes and imports
- **Deploy** — Docker Compose stack with a primary, two replicas, Redis, API and web

## Quickstart

```sh
cp .env.example .env
# change POSTGRES_PASSWORD, REPLICATION_PASSWORD, SESSION_SECRET, NODE_SECRET, ADMIN_PASSWORD
docker compose up --build -d
```

| Service | URL |
| --- | --- |
| Admin console | http://localhost:3000 |
| API + health | http://localhost:8080/v1/health |

Sign in with `ADMIN_EMAIL` / `ADMIN_PASSWORD`. The first boot migrates the database, registers
`DATABASE_URL` as the leader, and registers every DSN in `DATABASE_FOLLOWERS` as a follower.
The replicas bootstrap themselves with `pg_basebackup` on first start.

> `READ_ROUTING=replica` (the compose default) sends reads to healthy followers. Use `leader`
> when you need read-after-write consistency without any lag window.

## Local development

Backend:

```sh
cd backend
cat > .env <<'EOF'
DATABASE_URL=postgres://openwrite:openwrite@127.0.0.1:5432/openwrite
REDIS_URL=redis://127.0.0.1:6379
BIND_ADDR=127.0.0.1:8080
SESSION_SECRET=dev-session-secret-at-least-24-chars
ADMIN_EMAIL=admin@openwrite.local
ADMIN_PASSWORD=openwrite-dev-admin
EOF
cargo run
```

Frontend (proxies `/admin/api` and `/v1` to `OPENWRITE_API`, default `http://127.0.0.1:8080`):

```sh
cd web
npm install
npm run dev                 # http://localhost:3000
```

Tests:

```sh
cd backend && cargo test
cd web && npm run build
```

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `DATABASE_URL` | required | PostgreSQL DSN for the primary |
| `DATABASE_FOLLOWERS` | empty | Comma-separated read-replica DSNs registered on boot |
| `REDIS_URL` | `redis://127.0.0.1:6379` | Sessions, admin cookies |
| `BIND_ADDR` | `0.0.0.0:8080` | API listen address |
| `PUBLIC_URL` | `http://localhost:8080` | Public API origin |
| `DATA_DIR` | `./data` | Stores the encrypted `leader` file |
| `SESSION_SECRET` | required | Session hashing secret (24+ chars) |
| `NODE_SECRET` | falls back to `SESSION_SECRET` | AES-GCM key for node DSNs |
| `ADMIN_EMAIL` / `ADMIN_PASSWORD` | required | Bootstrap console admin (password 10+ chars, first boot only) |
| `CORS_ORIGINS` | empty | Comma-separated allowed origins |
| `COOKIE_SECURE` | `false` | Set `true` behind HTTPS |
| `COOKIE_SAMESITE` | `lax` | `lax`, `strict` or `none` |
| `READ_ROUTING` | `leader` | `leader` or `replica` |
| `IMPORT_MAX_DOCUMENTS` | `100000` | Per-import document cap |

Control tables are prefixed `ow_` (`ow_projects`, `ow_users`, `ow_databases`,
`ow_collections`, `ow_nodes`, `ow_admins`). Everything a collection stores lives in a real
table:

```
ow_<projectId>_<databaseId>.<collectionId>
  id text primary key,
  data jsonb not null,
  permissions jsonb not null,
  created_at timestamptz,
  updated_at timestamptz
```

Schemas are namespaced per project, so the same database and collection IDs can exist in two
projects. Names longer than PostgreSQL's 63-byte identifier limit fall back to
`ow_<sha256 prefix>`. You can query documents with plain SQL, indexes and joins.

## Appwrite compatibility

Send the usual headers: `X-Appwrite-Project`, plus `X-Appwrite-Key` for server access or the
`a_session_<projectId>` cookie for user sessions.

Implemented:

- `POST /v1/account`, `GET /v1/account`, name/password/prefs updates
- Email/password sessions: `POST /v1/account/sessions/email`, list, delete current/all
- `GET/POST /v1/users`, `GET/DELETE /v1/users/{id}` for server keys
- Databases and collections: list, create, get, delete
- Attributes: string, integer, float, boolean, email, url, ip, datetime, enum — create and delete
- Documents: list, create, get, patch, delete
- Document queries: `limit`, `offset`, `orderAsc`, `orderDesc`, `equal`, `notEqual`, `lessThan`,
  `lessThanEqual`, `greaterThan`, `greaterThanEqual`, `search`, `startsWith`, `endsWith`,
  `isNull`, `isNotNull`
- ACLs: `read(...)`/`write(...)` with `any`, `guests`, `users`, `user:<id>`, plus `create`,
  `update`, `delete` variants and the legacy `write` alias. API keys bypass ACL checks;
  collection-level and document-level security are both honored.

Not implemented yet: buckets, teams, messaging, functions, realtime, relationships, spatial
attributes, transactions, GraphQL, password recovery emails.

## Nodes and failover

The console's **Nodes** page lists every registered PostgreSQL node with health, replication
state, lag and write capability. Promotion is deliberately manual because Openwrite cannot fence
a database for you:

1. Let the target replica catch up, then run `pg_ctl promote` on it.
2. Stop writes on the old primary and fence it so it cannot accept them again.
3. Repoint the remaining replicas at the new leader (or stop them), since their
   `primary_conninfo` still names the old primary.
4. Click **Promote** in the console.

Openwrite then verifies the node accepts writes, marks the old node retired, rewrites the
encrypted leader file, and reloads routing. The API keeps serving from the saved leader across
restarts. Skipping the fencing step risks split-brain and data loss.

Promotion itself works while the old primary is already unreachable because admin sessions live
in Redis, not the database. Reads routed to replicas can be stale until those replicas are
repointed at the new leader; use `READ_ROUTING=leader` if you need strict read-after-write during
a failover.

## Importing from Appwrite

Console → **Import**, or `POST /admin/api/import` with:

```json
{
  "endpoint": "https://cloud.appwrite.io/v1",
  "apiKey": "standard_…",
  "remoteProjectId": "my-app",
  "targetProjectName": "My app (imported)"
}
```

The source key needs databases, collections, documents and users scopes. Imports copy
databases, collections, supported attributes, documents (original IDs, data, ACLs, timestamps)
and users. Existing IDs are skipped, so a failed run can be retried; the document cap marks the
run truncated.

**Passwords cannot be imported.** Appwrite never exposes password hashes, so imported users are
created with `password_hash = NULL` and flagged `imported`. They cannot sign in until an admin
sets a password in the console.

## Repository layout

```
backend/    Rust API, migrations, Dockerfile
web/        Next.js admin console, Dockerfile
deploy/     Replica bootstrap and replication init scripts
compose.yaml
```

## Known limitations

- A single Redis instance is a single point of failure for sessions.
- Failover fencing is an operator responsibility; promotion is not automatic. Followers keep
  their old upstream after a promotion until you repoint them.
- No backups, replication slots management or automatic replica re-seeding.
- Admin console auth is a single bootstrap admin account.
- Imported users need a password reset before they can authenticate.
