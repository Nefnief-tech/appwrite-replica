-- API request events feed the console usage dashboards.

create table if not exists ow_request_events (
    id bigint generated always as identity primary key,
    created_at timestamptz not null default now(),
    project_id text not null default '',
    method text not null,
    route text not null,
    status int not null,
    duration_ms int not null default 0,
    origin text not null default '',
    client text not null default '',
    user_agent text not null default ''
);

create index if not exists ow_request_events_created_idx
    on ow_request_events (created_at);

create index if not exists ow_request_events_project_idx
    on ow_request_events (project_id, created_at);

create index if not exists ow_request_events_origin_idx
    on ow_request_events (origin, created_at);
