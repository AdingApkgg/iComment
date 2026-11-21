-- 登录改成 OAuth：一个账号可以挂多个 provider 身份。
create table account_identity (
  id uuid primary key,
  account_id uuid not null references account(id) on delete cascade,
  provider text not null check (provider in ('google', 'github')),
  subject text not null,
  email text not null,
  created_at timestamptz not null default now(),
  unique (provider, subject)
);

create index account_identity_account_idx on account_identity (account_id);

-- 域名归属校验（IndexNow 式）：先在域名根下放 `<key>.txt`，服务端回抓比对通过才建站。
create table site_claim (
  id uuid primary key,
  account_id uuid not null references account(id) on delete cascade,
  origin text not null,
  claim_key text not null,
  attempts integer not null default 0,
  verified_at timestamptz,
  expires_at timestamptz not null,
  created_at timestamptz not null default now()
);

create index site_claim_owner_idx on site_claim (account_id, created_at desc);

-- 同一个 key 只能挂在一个待验证的申请上
create unique index site_claim_pending_key_idx
  on site_claim (claim_key)
  where verified_at is null;

-- 同一个账号对同一个域名只留一条待验证的申请
create unique index site_claim_pending_origin_idx
  on site_claim (account_id, lower(origin))
  where verified_at is null;
