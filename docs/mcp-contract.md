# MCP 契约 + install.md

agent 是主用户，所以开户、建站、验收都由它驱动，人只在授权那一下出现。

## 发现顺序

```
1. GET https://icomment.dev/llms.txt          → 指向 install.md 与 MCP endpoint
2. GET https://icomment.dev/install.md        → 机器可读安装流程（兜底，无需 MCP）
3. MCP: https://icomment.dev/mcp              → 首选路径
```

`install.md` 必须自足：即使客户端没有 MCP，照它走也能装完。MCP 只是省掉解析长文的 token。

## 认证

- 首次调用任意工具 → `402`/`401` 带 `next`：device authorization。
  - `POST /api/v1/agent/device` → `{ device_code, user_code, verification_uri }`
  - 工具把 `verification_uri` 与 `user_code` 原样返回并**告诉 agent 停下等人点**。
  - 轮询 `POST /api/v1/agent/token`，成功后返回 `tsa_` 前缀 bearer。
- 一次性是的：token 只在 agent 的流程里活着，可 `revoke`。
- token 必须 `site`-scoped：`site` token 只能动指定站点。

## 工具集（全部有等价 REST，MCP 可整体拆掉）

| 工具 | 语义 | 关键规则 |
|---|---|---|
| `start_login` / `finish_login` | 设备授权两段（起流程 / 轮询到 token） | 中间必须停下等人 |
| `whoami` | 当前账号与 token scope | |
| `create_site` | 建站，返回 `site_key` + token + snippet | 同 origin 重复调用返回既有站点 |
| `list_sites` / `update_site` | 列站点 / 改设置 | 站点 token 只能动自己 |
| `verify_install` | **服务端自己抓页面验收** | 见下 |
| `list_threads` / `site_stats` | 按 path 列评论 / 看计数 | 游标分页 |
| `list_comments` / `create_comment` / `moderate_comment` | 审核与回填 | 需站点 scope |

`create_site` 返回的 `embed_snippet` 形如（`v=` 是版本号，用来破浏览器缓存）：

```html
<script src="https://comment.example.com/widget/v1/icomment.js?v=0.1.2"
        data-site-key="SITE_KEY" defer></script>
<ic-comment site-key="SITE_KEY" path="/posts/hello"></ic-comment>
<noscript>评论区需要 JavaScript</noscript>
```

`<ic-comment>`（不是 `<icomment>`：后者不是合法的自定义元素名）。标签名与 `path` 也可省略，脚本会按 `data-site-key` 自动插入并取当前路径，但显式写出来的页面更容易通过验收。

纯 headless 调用方一行 JS 都不用加载，直接打 `/api/v1/comments`。

## verify_install：整个流程的关键

没有它，AI 会以为装好了然后收工，用户看到空评论区，这一单就丢。

```
POST /api/v1/agent/sites/{site_id}/verify { url }
  1. 服务端 GET url（UA: icomment-verify/1，8s，最多 3 跳）
  2. 在 HTML 里查四要素：script、ic-comment 元素、site_key、path
  3. 逐项报缺并给出可直接照做的 hint
```

返回必须是可行动的三选一（`status` 由 `#[serde(tag = "status")]` 扁平输出）：

- `ok`：`{ "status": "ok", "found": ["script","element","site_key","path"], "http_status": 200 }`
- `partial`：`{ "status": "partial", "missing": ["site_key"], "hint": "元素上补 site-key=\"…\"" }` → agent 能自己补
- `failed`：`{ "status": "failed", "reason": "timeout" | "unreachable" | "page_error" | "too_many_redirects", "hint": "…" }`

控制台站点页也有同一个动作（`/dashboard/sites/{id}?verify={url}`），人不用开 MCP 就能自查。本地调试给核心加 `IC_VERIFY_PRIVATE=1` 才允许抓回环地址。

**验收只读用户公开页面，不执行用户 JS**（除第 3 步的探测回执），避免 SSRF 放大：目标 URL 必须公网可解析、禁 `169.254/16` 与 `127/8`、超时 8s、跟随最多 3 跳。

## 错误约定

```json
{ "type": "about:blank", "code": "site_not_found", "title": "site not found",
  "next": "call site.create first" }
```

`code` 是封闭枚举，agent 靠它分支，不靠 `title` 文案。

## 限流

`account.register` 10/小时/IP；`site.create` 20/天/账号；`verify_install` 30/小时/site；读接口按 site 配额。
