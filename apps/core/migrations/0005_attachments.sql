-- 评论附图：每条评论最多几张，靠 (comment_id, position) 保序。

create table attachment (
  id uuid primary key,
  -- 上传时还没评论，绑定后才填
  comment_id uuid references comment(id) on delete cascade,
  site_id uuid not null references site(id) on delete cascade,
  position int not null,
  path text not null,
  bytes int not null,
  width int not null,
  height int not null,
  created_at timestamptz not null default now(),
  unique (comment_id, position)
);

create index attachment_site_created on attachment (site_id, created_at);
