# 路线与验收

## 阶段

**S0 自己用（1 周）**
一个站（PG 单容器就够），只为验证「AI 从零装成功」这条链到底顺不顺。MCP + snippet + verify 三件套必须齐。
验收：把自己的站交给一个干净 agent 会话，不改 prompt，装成功。

**S1 公开（2–3 周）**
PG + Redis、托管、device auth、llms.txt + install.md 上线、widget 发 CDN v1、独立 embed 页。
验收：三个不同框架（Astro / Next / 纯 HTML）各由 agent 装上并通过 `verify_install`。

已完成：llms.txt + install.md（`apps/core/src/discover.rs`）、embed 页、widget 失败降级链接。

**S2 用得住（3–4 周）**
审批台、审计表回看、Akismet、贴图与反应（从 mikiacg 搬）、i18n。

已完成：审批台、审计留痕回看（`/dashboard/sites/{id}/audit` 与 `GET /api/v1/agent/sites/{id}/audit`，`moderation_event` 记录 actor/action/reason，用例 `every_moderation_step_leaves_a_trace`）。另加站点级域名黑名单（`site.blocked_domains`，命中含子域即进垃圾箱，控制在 20 条内），以及 Akismet 兜底（可选开关、3 秒超时、失败按本地结论走）、评论反应（五种固定表情，`/comments/{id}/reactions/{kind}` 的 POST/DELETE，匿名者发签名 `ic_voter` cookie 防重复计数，widget 只展示已有人投的表情）。附图：`POST /api/v1/uploads` 收 multipart（只认魔数，PNG/JPEG/WebP/GIF，≤4MiB，每 IP 每小时 40 张），回 `![](ic-attachment:<id>)` 占位符，提交评论时展开成绝对 `/media/<file>` 并在事务里绑定（每条 ≤4 张，一张只能绑一次），`/media` 带 ETag 与 immutable 长缓存，未绑定的图一天后由后台每小时扫掉，用例 `upload_then_comment_binds_the_image` 与 `attachment_rules_are_enforced`。
验收：一周内真实站点的 spam 全部落在 pending，零穿透到可见。

**S3 再谈收费**
额度字段已经在了，加 Stripe 与配额闸。之前不设任何限制。

## 每阶段的硬门

- S1 前：`verify_install` 在三个框架的失败态都能给出可行动 `hint`（不是只说失败）。✅ 判定拆成 script/site_key/defer/element/path 五项，缺哪项就报哪项并给改法（含整段 snippet），页面用别的 key 会点名；`scan` 单测 4 例 + 冒烟 `verify_reports_a_private_host_with_an_actionable_hint`、`verify_rejects_a_relative_url`。
- S2 前：widget gzip ≤ 15KB，Lighthouse a11y ≥ 95。✅ gzip 11.5KB（加反应后 12.35KB，仍在 15KB 内）；Lighthouse accessibility 100（`deploy/demo/index.html`：补 label + `aria-live` + `aria-busy` + `prefers-reduced-motion`，演示页 byline 对比度 3.07→4.6）。
- 上线前：`/embed` 页在 JS 禁用下可读可写。✅ `apps/core/src/embed.rs`，冒烟用例 `embed_page_works_without_javascript` 覆盖读、写、错误回显。
- 上线前：全部写接口有幂等键，重放测试通过。✅ `Idempotency-Key` 头与 `idempotency_key` 字段等价，`idempotency_header_prevents_double_post` 验证重放返回同一行且 `created=false`。

## 不做（本季）

WordPress 插件、OAuth 登录、实时 socket、自建 ML 反垃圾、移动端 SDK、多语言后台 UI。

## 风险与对策

| 风险 | 对策 |
|---|---|
| agent 装失败率高 → 用户看到空评论区 | verify 三态 + 可行动 hint；S0 阶段用干净会话反复压 |
| spam 直打 API | 默认 pending + Turnstile + 审计，见 `antispam-auth.md` |
| 全免费无上限被刷存储 | 单评论 8KB 上限；单站日写入软顶 1000 条（超了强制进审核队列，不拒收，`antispam::DAILY_SOFT_CAP`，用例 `daily_soft_cap_forces_review_but_keeps_accepting`） |
| Lit 被站点 CSP 拦 | snippet 给带 nonce 占位注释，install.md 里写清怎么加 |
| AGPL 吓退企业 | widget/sdk/mcp 是 MIT，只有 core 是 AGPL；README 首行写明 |
| 与 Giscus/Waline 同质 | 唯一差异点是 agent 自助路径，所有文案都从这条讲 |
