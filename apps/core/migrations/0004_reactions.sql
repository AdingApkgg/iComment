-- 反应：固定表情集合，一条评论每人对每种只算一次。

create type reaction_kind as enum ('up', 'heart', 'haha', 'thinking', 'party');

create table reaction (
  comment_id uuid not null references comment(id) on delete cascade,
  site_id uuid not null references site(id) on delete cascade,
  kind reaction_kind not null,
  voter text not null,
  created_at timestamptz not null default now(),
  primary key (comment_id, kind, voter)
);

create index reaction_comment on reaction (comment_id);
