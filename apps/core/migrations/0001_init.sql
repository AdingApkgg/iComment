-- 初始 schema。多租：所有业务行带 site_id。

create table account (
  id uuid primary key,
  email text not null unique,
  status text not null default 'active' check (status in ('active', 'suspended')),
  created_at timestamptz not null default now()
);

create table site (
  id uuid primary key,
  owner_id uuid not null references account(id) on delete cascade,
  origin text not null,
  name text,
  site_key text not null unique,
  site_secret text not null,
  require_approval boolean not null default true,
  challenge_mode text not null default 'turnstile' check (challenge_mode in ('turnstile', 'off')),
  widget_version text not null default '1',
  created_at timestamptz not null default now(),
  deleted_at timestamptz
);

-- origin 归一化后唯一（小写、去尾斜杠、https 优先）
create unique index site_origin_live on site (lower(origin)) where deleted_at is null;

create table agent_token (
  id uuid primary key,
  account_id uuid not null references account(id) on delete cascade,
  site_id uuid references site(id) on delete cascade,
  token_hash text not null unique,
  label text,
  last_used_at timestamptz,
  revoked_at timestamptz,
  created_at timestamptz not null default now()
);

create table device_code (
  device_code text primary key,
  user_code text not null,
  account_id uuid references account(id) on delete set null,
  status text not null default 'pending' check (status in ('pending', 'approved', 'denied')),
  created_at timestamptz not null default now(),
  expires_at timestamptz not null
);

create table thread (
  id uuid primary key,
  site_id uuid not null references site(id) on delete cascade,
  path text not null,
  title text,
  comment_count integer not null default 0,
  last_at timestamptz,
  created_at timestamptz not null default now(),
  unique (site_id, path)
);

create table comment (
  id uuid primary key,
  site_id uuid not null references site(id) on delete cascade,
  thread_id uuid not null references thread(id) on delete cascade,
  parent_id uuid references comment(id) on delete set null,
  body_md text not null,
  body_html text not null,
  author_kind text not null default 'anon' check (author_kind in ('anon', 'signed', 'owner')),
  author_uid text,
  author_name text,
  author_avatar text,
  author_email text,
  status text not null default 'pending'
    check (status in ('pending', 'visible', 'hidden', 'spam', 'deleted')),
  identity_unverified boolean not null default false,
  ip_hash text,
  ua_hash text,
  idempotency_key text,
  created_at timestamptz not null default now()
);

create index comment_thread_visible on comment (thread_id, created_at) where status in ('visible', 'pending');
create unique index comment_idempotency on comment (site_id, idempotency_key)
  where idempotency_key is not null;

create table moderation_event (
  id uuid primary key,
  site_id uuid not null references site(id) on delete cascade,
  comment_id uuid references comment(id) on delete set null,
  actor_kind text not null,
  actor_id text,
  action text not null,
  reason text,
  created_at timestamptz not null default now()
);

create index moderation_event_site on moderation_event (site_id, created_at desc);
