# 数据模型

多租，所有业务行带 `site_id`。PG，行级约束，不用软删除以外的技巧。

```sql
-- 租户
site(id, owner_id, origin, name, site_key, site_secret,
     require_approval bool default true, challenge_mode enum('turnstile','off') default 'turnstile',
     widget_version text default '1', created_at, deleted_at)

account(id, email unique, status enum('active','suspended'), created_at)
agent_token(id, account_id, site_id null, token_hash, label, last_used_at, revoked_at)
device_code(device_code, user_code, account_id null, status, expires_at)

-- 内容
thread(id, site_id, path, title null, comment_count, last_at,
       unique(site_id, path))
comment(id, site_id, thread_id, parent_id null, body_md, body_html,
        author_kind enum('anon','signed','owner'), author_uid null, author_name null,
        author_avatar null, author_email null,
        status enum('pending','visible','hidden','spam','deleted'),
        identity_unverified bool default false,
        ip_hash, ua_hash, idempotency_key null, created_at,
        unique(site_id, idempotency_key))

-- 审计与运营
moderation_event(id, site_id, comment_id, actor_kind, actor_id, action, reason, created_at)
rate_bucket  -- Redis 为主，PG 侧只留日汇总
```

## 约束

- `site.origin` 归一化后唯一（小写、去尾斜杠、`https` 优先）。
- `comment.parent_id` 必须同 thread；只做一层嵌套（回复平铺到父级下，不递归）。
- `path` 归一化：去查询串、去尾斜杠、`/` 兜底；`thread` 唯一键建立在归一化值上。
- `idempotency_key` 命中时返回原记录，不新建。
- 删除 = `status='deleted'` + 体清空，计数字段同步 `-1`。物理删除由维护任务在 30 天后做。
- `site_key` 可公开；`site_secret` 只在签名校验时用，永不出现在响应里（`site.get_snippet` 只给 key）。

## 计数

`thread.comment_count` 与 `last_at` 用写路径同事务更新，不做异步补偿，避免 widget 上「有 N 条却看不到」。
