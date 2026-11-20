# 反垃圾 + 身份

无头 API 最大的运营风险：**所有 spam 直接打 API，没有页面稀释**。所以反垃圾不能事后补。

## 三层

**1. 默认关（safe by default）**

新站默认 `require_approval = true`：匿名评论进 pending，站主/agent 批准后可见。列表接口默认只返可见项，`?include_pending=1` 需 site token。

**2. 验证挑战**

匿名写必须选一样：

- **Turnstile**（首选，默认开）——token 随评论提交，服务端校验；
- 或 `signed identity`（见下）——已验身份的通道免挑战；
- 或站点关闭挑战（仅在开了审批时允许）。

挑战失败回 `403 { code: "challenge_failed" }`，agent 可重取 token 重试，不消耗幂等键。

**3. 内容规则**

- 链接数量（>3 转 pending）、最小长度、重复体（同 site+path 的 10 分钟内同文合并）。
- 全站黑名单（邮箱域、IP 段）、Akismet 适配器（可选开关，不自建模型）。
- 存储前做一次 Honeypot 字段校验。

## signed identity（HMAC 透传）

已登录的站点服务端签短时 token，评论者身份跟评论一起进来，免挑战：

```
payload = base64url(JSON{ site, uid, name, avatar, iat, exp })   // exp-iat ≤ 300s
sig     = base64url(HMAC-SHA256(site_secret, payload))
POST /api/v1/threads/{path}/comments
X-IC-Identity: <payload>.<sig>
```

- 验签失败不拒，静默降级成匿名（留 `identity_unverified` 标记）。
- `uid` 只在 site 内可见，跨站不合并。
- `site_secret` 只出现在服务端，`site_key` 可以公开。

## 边界

- **不做**：邮箱验证码评论、密码账号、OAuth 社交登录（首版）。评论者身份要么来自站点透传，要么就是匿名。
- **不做**：自建 ML 模型。规则 + Turnstile + Akismet 足够到有量的那天。
- 所有判定写审计表 `moderation_event`，agent/人能复盘为什么被拦——避免「被吞了没人知道」。
