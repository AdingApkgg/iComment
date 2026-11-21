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
- 站点级域名黑名单（`site.blocked_domains`，最多 20 条，命中含子域即判 spam；`a.com` 同时覆盖 `cdn.a.com`）。
  - 由站点主人在设置面板或 `update_site` 维护，agent 可代填；传 `[]` 清空。
  - 命中是**直接 spam 不发挑战**，所以要先确认对方确实只在链接里出现。
- Akismet 兜底（可选，不自建模型）：配 `IC_AKISMET_KEY` 才生效，`IC_AKISMET_ENDPOINT` 可指向自建代理。
  - 只问本地规则已放行、且宿主站没透传**验签通过**身份的评论；命中直接判 spam。
  - 3 秒超时，连不上或回不认识的内容一律按本地结论走，**绝不因为外部抖动丢评论**。
  - 外部回的 `true/false` 之外的字符串（如 `Invalid API key`）算"没意见"，同时打一条 warn 日志。
- 存储前做一次 Honeypot 字段校验。

## signed identity（HMAC 透传）

已登录的站点服务端签短时 token，评论者身份跟评论一起进来，免挑战：

```
payload = base64url(JSON{ site, uid, name, avatar, iat, exp })   // exp-iat ≤ 300s
sig     = base64url(HMAC-SHA256(site_secret, payload))
POST /api/v1/comments
X-IC-Identity: <payload>.<sig>
```

不用自己拼：`POST /api/v1/agent/sites/{site_id}/identity` 传 `{uid, name}` 就返回整套（`header` + `expires_in`），SDK 是 `signIdentity()`，MCP 是 `sign_identity`。

- 验签失败不拒，静默降级成匿名（留 `identity_unverified` 标记），消息照常入库。
- 验过就免人机验证，且即使站点开了审核也直接 `visible`（除非被内容规则判成 spam）。
- `uid` 只在 site 内可见，跨站不合并。
- `site_secret` 只出现在服务端，`site_key` 可以公开；`SiteView` 把它回给 owner 侧接口。
- 客户端不能自己伪造：改签名任一字节即降级，冒烟用例 `signed_identity_skips_review_and_challenge` 覆盖三个状态。

## 边界

- **不做**：邮箱验证码评论、密码账号、OAuth 社交登录（首版）。评论者身份要么来自站点透传，要么就是匿名。
- **不做**：自建 ML 模型。规则 + Turnstile + Akismet 足够到有量的那天。
- 所有判定写审计表 `moderation_event`，agent/人能复盘为什么被拦——避免「被吞了没人知道」。
