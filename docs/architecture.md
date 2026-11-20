# 架构

## 定位

iComment = 开源 SaaS 评论服务。三种消费者，按优先级：

1. **agent**（一等公民）：通过 MCP 自助开户、建站、取 snippet、验收。没有人坐在旁边。
2. **headless 调用方**：直接打 REST（无 MCP 的客户端、SSR、自家前端）。
3. **人**：在默认 Lit widget 上读和写评论。

「核心只要能调 api」——REST 是唯一权威接口；MCP 是同权限、同限流的一层薄封装，可以整体拆掉而服务不受影响。

## 组件

```
apps/core          Rust / axum
  /api/v1/*        REST：权威接口（公开读/写 + agent 面）
  /widget/v1/*     内嵌 widget 产物（构建期 include_bytes，单二进制自带）
  /dashboard /sites 等  控制台：登录、建站、审核、令牌、device 页
packages/widget    Lit + Shadow DOM，构建产物同时供 CDN 分发，MIT
packages/sdk       带类型的 API 客户端（TS），MIT
packages/mcp       MCP server：stdio 或 `--http` 独立进程，MIT
deploy/            Dockerfile + compose + Caddyfile + 演示页
```

**core 不依赖 widget 源码也不依赖 mcp。** mcp 只调 REST（默认本机回环），不直连数据库——保证两条入口行为一致。widget 只在构建期被 bundle 脚本塞进 `apps/core/assets/`，运行时是静态字节。

MCP 刻意不进 core 进程：它是可整体拆掉的壳，独立部署也就不必给 Rust 侧引 MCP 依赖。

## 关键决策

| 决策 | 取值 | 理由 |
|---|---|---|
| 后端 | Rust + axum | 小二进制、快冷启动（iComment 原判断：Artalk 62MB 二进制 / 430 Go 依赖是反面教材） |
| 存储 | PostgreSQL | 多站多租，需要行级约束与并发写入；不用 SQLite 是为了 SaaS 形态 |
| 缓存/限流 | Redis | 限流桶、验证码挑战、列表缓存 |
| widget | Lit + Shadow DOM | 无框架依赖、样式天然隔离、可被搜索爬到；iframe 会被尺寸和 SEO 惩罚 |
| 分发 | CDN，路径带主版本 `@1/` | 破坏性升级不打断已嵌站点 |
| 渲染 | 默认 widget **可选**，安装时问 | 纯 headless 用户不加载任何 JS |
| 许可证 | core AGPL-3.0；widget/sdk/mcp MIT | AGPL 挡托管白嫖，MIT 保证嵌入无摩擦 |
| 收费 | 先纯公益全免费 | 池子养大再谈；额度表先留字段不设闸 |

## 接口原则

- REST 一等，MCP 只是投影；任何 MCP 工具都必须有等价 REST 端点。
- 公开读（评论列表）允许 CORS + 强缓存，服务端可 SSR 取数——这是 headless 相对 iframe 唯一的真优势，别浪费。
- 写（发评论）必须带身份或不带身份但有验证门槛，见 `antispam-auth.md`。
- 所有响应带 `X-IC-Version`；错误统一 RFC 9457 `application/problem+json`，附机器可读 `code`，让 agent 能自行纠正而不是猜。
- 幂等：`create_site`、`create_comment` 接受 `Idempotency-Key`。

## 反脆弱

- widget 加载失败必须降级为纯链接（`/embed/<site>/<path>` 独立页），不留空白块。
- widget 目标体积 ≤ 15KB gzip；超出就砍功能。
- MCP 认证失败返回可执行指引（`{ code: "unauthenticated", next: "<authorize_url>" }`），不是 401 空体。

## 明确不做

Rust 后端里不塞前端构建链；widget 独立仓库构建后发 CDN。不提供 WordPress 插件、不做 iOS/Android SDK、不做实时 socket（首版轮询即可）。
