-- 附图改内容寻址（CAS）：文件名是内容的 sha256，同样的图只存一份。
-- `blob` 只登记内容和类型；谁在用由 `attachment.hash` 表示，没人引用超过一天才删。

create table blob (
  hash text primary key,
  bytes int not null,
  ext text not null,
  created_at timestamptz not null default now()
);

-- 清理任务按“没人引用 + 够老”扫，给这个条件留索引
create index blob_orphans on blob (created_at);

-- 老附件存的是随机文件名，改成 hash；迁移前传的图没有 hash 可算，清掉重传即可
delete from attachment;
alter table attachment drop column path;
alter table attachment add column hash text not null references blob (hash);
create index attachment_hash on attachment (hash);
