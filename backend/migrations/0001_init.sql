create extension if not exists pgcrypto;

create table if not exists ow_admins (
    id uuid primary key,
    email text not null unique,
    name text not null default 'Admin',
    password_hash text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create table if not exists ow_projects (
    id text primary key,
    name text not null,
    api_key_hash text not null,
    api_key_prefix text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create table if not exists ow_users (
    id text not null,
    project_id text not null references ow_projects(id) on delete cascade,
    email text not null,
    name text not null default '',
    password_hash text,
    prefs jsonb not null default '{}'::jsonb,
    status boolean not null default true,
    imported boolean not null default false,
    email_verified boolean not null default false,
    registration timestamptz not null default now(),
    password_update timestamptz not null default now(),
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    primary key (project_id, id),
    unique (project_id, email)
);

create index if not exists ow_users_project_idx on ow_users(project_id);

create table if not exists ow_databases (
    id text not null,
    project_id text not null references ow_projects(id) on delete cascade,
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    primary key (project_id, id)
);

create index if not exists ow_databases_project_idx on ow_databases(project_id);

create table if not exists ow_collections (
    id text not null,
    project_id text not null,
    database_id text not null,
    name text not null,
    attributes jsonb not null default '[]'::jsonb,
    permissions jsonb not null default '[]'::jsonb,
    document_security boolean not null default true,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    primary key (project_id, database_id, id),
    foreign key (project_id, database_id)
        references ow_databases(project_id, id) on delete cascade
);

create table if not exists ow_nodes (
    id uuid primary key,
    name text not null unique,
    dsn text not null,
    role text not null check (role in ('leader', 'follower', 'retired')),
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create unique index if not exists ow_nodes_one_leader
    on ow_nodes ((role)) where role = 'leader';
