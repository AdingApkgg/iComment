//! 给 agent 的发现入口：`/llms.txt` 与 `/install.md`。
//!
//! MCP 客户端在拿到 URL 后先读这两处来决定怎么装、调哪个 base。内容全部静态，
//! 只有 `IC_PUBLIC_BASE_URL` 会拼进去。

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};

use crate::state::AppState;

pub async fn llms(state: State<AppState>) -> Response {
    let base = &state.cfg().public_base_url;
    let body = format!(
        r#"# iComment

自托管评论服务。REST + MCP，无需前端框架。

## 接口

- REST: {base}/api/v1
- MCP (stdio): `npx -y @icomment/mcp`，HTTP 传输 `/mcp`
- 安装说明: {base}/install.md

## 上手

1. `POST {base}/api/v1/agent/device` 拿设备码，把 `verification_uri_complete` 交给站点主人在浏览器里确认
2. 轮询 `POST {base}/api/v1/agent/token` 直到拿到以 `tsa_` 开头的 token
3. `POST {base}/api/v1/agent/sites` 建站，响应里的 `embed_snippet` 原样贴进 HTML
4. `POST {base}/api/v1/verify` 校验装好了没

账号级 token 可管所有站；每个站另有站点级 token，只够读写自己。
"#
    );
    text(body, "text/plain; charset=utf-8")
}

pub async fn install(state: State<AppState>) -> Response {
    let base = &state.cfg().public_base_url;
    let body = format!(
        r#"# 安装 iComment

三件事：拿 token、建站、贴脚本。

## 1. 拿 token

```sh
curl -s {base}/api/v1/agent/device -X POST
```

响应里的链接打开确认，然后：

```sh
curl -s {base}/api/v1/agent/token -X POST \
  -H 'content-type: application/json' \
  -d '{{"device_code":"<上一步的 device_code>"}}'
```

拿到的 `token` 存好，别贴进前端。

## 2. 建站

```sh
curl -s {base}/api/v1/agent/sites -X POST \
  -H 'authorization: Bearer <token>' \
  -H 'content-type: application/json' \
  -d '{{"origin":"https://example.com"}}'
```

响应里两样东西：`site_key`（公开，写进脚本）和 `embed_snippet`（原样贴进页面）。`site_token` 只够读写本站，适合给 CI 或子 agent。

## 3. 贴脚本

```html
<div id="comments"></div>
<script src="{base}/widget/v1/icomment.js" data-site-key="<site_key>" defer></script>
```

## 4. 校验

```sh
curl -s {base}/api/v1/verify -X POST \
  -H 'authorization: Bearer <token>' \
  -H 'content-type: application/json' \
  -d '{{"site_id":"<站点 id>","url":"https://example.com/posts/hello"}}'
```

`verdict` 为 `ok` 才算装好；`partial` 会列出缺哪一项。

## 不用 JS 的读者

`{base}/embed/<site_key>/<path>` 是服务端渲染的评论区，能读能写，适合脚本被拦或访客关了 JS。
"#
    );
    text(body, "text/markdown; charset=utf-8")
}

fn text(body: String, mime: &'static str) -> Response {
    ([(header::CONTENT_TYPE, mime)], body).into_response()
}
