-- Password recovery, email verification and email-MFA challenge secrets.
-- Only hashes are stored; the plaintext secret travels in the email link/code.

create table if not exists ow_auth_tokens (
    id uuid primary key default gen_random_uuid(),
    project_id text not null references ow_projects(id) on delete cascade,
    user_id text not null,
    kind text not null,
    secret_hash text not null,
    expires_at timestamptz not null,
    used_at timestamptz,
    created_at timestamptz not null default now()
);

create index if not exists ow_auth_tokens_lookup_idx
    on ow_auth_tokens (project_id, user_id, kind, secret_hash);
