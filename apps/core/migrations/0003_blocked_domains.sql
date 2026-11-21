-- 站点级域名黑名单：评论里出现这些域名的链接就直接进垃圾箱
alter table site add column blocked_domains text[] not null default '{}';
