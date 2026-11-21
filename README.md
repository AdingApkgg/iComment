# iComment

开源自托管评论系统，为「agent 自助装站」设计：一句 `claude mcp add` + 一次设备授权，站点就能长出自己的评论区。

Artalk / Waline / Twikoo 这一类先把「人」放在流程中间：进后台、开站点、手抄 snippet。iComment 把这三步交给 agent，人只在一处点确认。Rust + axum 单二进制，内嵌 widget 与迁移，前端是一份 12 KB 的 Lit Web Component，数据全在你自己库里。

- 核心 AGPL-3.0；`packages/widget`、`packages/sdk`、`packages/mcp` 是 MIT —— 要塞进别人网站里的东西不能有传染性。
- 默认 headless：REST 是唯一权威接口，MCP 只是同权限的一层薄封装，删掉它服务照跑。
- 先纯公益全免费，额度字段留着不设闸。

## 目录

```text
apps/core          Rust / axum：REST + 控制台 + 内嵌 widget + 迁移
packages/widget    Lit + Shadow DOM 评论组件（MIT）
packages/sdk       带类型和 problem+json 解析的 API 客户端（MIT）
packages/mcp       MCP server：device 登录、建站、管评论（MIT）
deploy             Dockerfile / compose / Caddyfile
docs               架构、MCP 契约、反垃圾、数据模型、widget、路线
```

## 本地跑

需要 Rust 1.94+、bun、PostgreSQL 18、Redis。

```bash
docker run -d --name ic-db -p 25493:5432 -e POSTGRES_USER=icomment \
  -e POSTGRES_PASSWORD=icomment -e POSTGRES_DB=icomment postgres:18-alpine
docker run -d --name ic-redis -p 26493:6379 redis:8-alpine

# 附图要 SeaweedFS（CAS 存储）：可只起 master/volume/filer，把地址指向它；
# 不跑就留空 IC_SEAWEED_FILER_URL，附图只落本地目录或内存
docker compose -f deploy/compose.dev.yml up -d
export IC_SEAWEED_FILER_URL=http://127.0.0.1:18888/uploads

export IC_DATABASE_URL=postgres://icomment:icomment@127.0.0.1:25493/icomment
export IC_REDIS_URL=redis://127.0.0.1:26493

bun install
./scripts/bundle-widget.sh       # 把 widget 产物塞进 apps/core/assets，供单二进制内嵌
cargo run -p icomment-core       # 迁移在启动时自动执行，打开 http://127.0.0.1:8080
```

首次打开控制台注册账号，创建站点，页面会给出一段 `<script>` snippet，贴到主题里即可。

## 给 agent 装

```bash
claude mcp add icomment -- npx -y @icomment/mcp   # 或本地：node packages/mcp/dist/main.js
```

然后让它 `start_login`：终端给出四位码，在 Web 控制台 `/device` 输入一次。之后的 `create_site` 会把 snippet 一起返回，agent 自己贴进模板并跑 `verify_install` —— 服务端会真的去抓那一页，缺 `script` / `ic-comment` / `site-key` / `path` 哪一项就报哪一项，附可直接照做的提示。

不想给 MCP 也一样，REST 就够：

```bash
curl -X POST $BASE/api/agent/sites/$SITE_ID/verify \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"url":"https://your.site/posts/hello"}'
```

## 部署

```bash
cp deploy/.env.example deploy/.env    # 填域名、密码，openssl rand -hex 32 生成 IC_SESSION_SECRET
docker compose -f deploy/compose.yml --env-file deploy/.env up -d --build
```

Caddy 自动签 HTTPS 并反代到核心；PG / Redis 只在 compose 内网可达，不对外暴露端口。镜像基于 distroless（约 55 MB，无 shell），健康检查走 `icomment-core health` 子命令。

走 Cloudflare Tunnel（本机 80/443 被占用、或不想开端口）就叠 `-f deploy/compose.tunnel.yml`，Caddy 只听回环明文，TLS 由边缘终止——步骤见 [`docs/deploy.md`](docs/deploy.md)。线上部署是 `https://icomment.saop.cc`：`/` 落地页、`/dash` 控制台、`/api` 接口。

证书：默认走 Let's Encrypt，需要域名已经解析到这台机器且 80/443 可达。内网或还没解析时叠加 `-f deploy/compose.local.yml`，改用自签证书。宿主 80/443 已被别的反代占用就设 `IC_HTTP_PORT` / `IC_HTTPS_PORT`。

人机验证可选：填 `IC_TURNSTILE_SECRET` 与 `IC_TURNSTILE_SITE_KEY` 才生效，站点维度三态 `off / suspicious / always`，默认 `suspicious`（可疑才拦）。

Akismet 兜底也可选：只填 `IC_AKISMET_KEY` 就生效，只对本地规则放行、且没有可信身份的评论发问；3 秒超时，失败按本地结论走。

## 接口速览

| 面                 | 前缀                                 | 鉴权                                             |
| ------------------ | ------------------------------------ | ------------------------------------------------ |
| widget 公开读 / 写 | `/api`                               | `x-site-key` 或 `Origin`                         |
| agent              | `/api/agent`                         | `Authorization: Bearer tsa_…`（站点级或账号级）  |
| 设备授权           | `/api/agent/device`、`/agent/token`  | 无需                                             |
| 控制台             | `/login`、`/dash`、`/sites`          | 签名 cookie                                      |
| 审核留痕           | `/dash/sites/{id}/audit`             | 每次放行/隐藏/删除都记一条，可按评论回看         |
| 域名黑名单         | 站点设置面板 / `update_site`         | 命中即进垃圾箱，子域一并拦；关键词与链接数仍兜底 |
| Akismet            | `IC_AKISMET_KEY`（可选）             | 本地放行后再问一次；超时或报错按本地结论走       |
| 附图               | `POST /api/uploads`、`/media/{hash}` | 上传回占位符；内容寻址，`/media` 长缓存不可变    |

错误统一 `application/problem+json`，带 `code`、`fields`，必要时 `next` 指引下一步；限流回 `Retry-After`。

重试安全：`POST /api/comments` 收 `Idempotency-Key` 头（或 body 里的 `idempotency_key`），重放返回同一行且 `created=false`。

反应：`POST|DELETE /api/comments/{id}/reactions/{kind}`，`kind` 取 `up|heart|haha|thinking|party`。匿名访客首次投票会拿到签名 `ic_voter` cookie，同一人对同一表情只计一次。

附图按内容寻址（CAS）：`POST /api/uploads`（multipart，字段 `file`，≤4MiB，PNG/JPEG/WebP/GIF）回 `markdown` 片段，贴进 `body_md` 即可，提交评论时展开成 `/media/<sha256>`；每条最多 4 张，每 IP 每小时 40 张。只按魔数判类型（不收 SVG），同样的图只存一份（文件名就是内容哈希，天然去重、可长缓存）。字节存 SeaweedFS 的 filer（`IC_SEAWEED_FILER_URL`，compose 里已带 master/volume/filer 与数据卷），本地开发可退回本地目录（`IC_UPLOAD_DIR`），一天没被引用的自动清理。

无 JS 兜底：`/embed/<site_key>/<path>` 是服务端渲染的评论区，原生表单就能发；widget 拿不到元信息时也直接给这个链接。agent 可以先读 `/llms.txt` 与 `/install.md` 自己装。

## 开发约定

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bunx biome check .
bunx tsc -b packages/mcp          # sdk 走 project references，一起编译
```

`apps/core/assets/` 是构建产物，由 `scripts/bundle-widget.sh` 生成，不进 biome。

### 冒烟测试

`apps/core/tests/smoke.rs` 跑真实链路：设备授权 → 建站 → 发评论 → 审核 → 留痕回看 → 黑名单拦截 → 撤销。
它连真 PG 与 Redis，**每个用例自建 schema 并在结束时删掉**，靠这两个变量定位：

```bash
IC_TEST_DATABASE_URL=postgres://icomment:icomment@127.0.0.1:25493/icomment \
IC_TEST_REDIS_URL=redis://127.0.0.1:26493 \
cargo test -p icomment-core --test smoke
```

变量缺失时用例静默跳过，所以 CI 会额外断言「没有 0 passed」。用例之间用伪造的
`X-Forwarded-For` 分桶，互相不抢限流名额。

## 文档

`docs/architecture.md` 起手，往下是 `mcp-contract.md`、`antispam-auth.md`、`data-model.md`、`widget.md`、`roadmap.md`。

## 许可

核心 `apps/core` 与 `deploy` 为 AGPL-3.0-or-later，见 `LICENSE`；`packages/*` 为 MIT，见各自 `LICENSE`。
