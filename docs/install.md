# install.md

机器可读安装流程：没有 MCP 客户端时照这份走也能装完。所有步骤都是 REST，`$BASE` 是实例地址。

## 1. 拿凭据（device authorization）

```bash
curl -sX POST $BASE/api/agent/device
# { "device_code": "…", "user_code": "ABCD-2345", "verification_uri": "$BASE/device", "expires_in": 900 }
```

**停下来**，把 `verification_uri` 和 `user_code` 交给用户，等他在浏览器里输入并确认。

然后轮询：

```bash
curl -sX POST $BASE/api/agent/token -H 'content-type: application/json' \
  -d '{"device_code":"…"}'
# 202/401 device_authorization_pending  → 继续等，间隔 5s
# 200 { "token": "tsa_…", "account_id": "…" }
```

之后所有请求带 `authorization: Bearer tsa_…`。

## 2. 建站

```bash
curl -sX POST $BASE/api/agent/sites -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"origin":"blog.example.com","name":"我的博客"}'
```

`origin` 只写域名或带 scheme 均可，服务端会归一化。响应里的 `site_token` 只能动这个站点，`embed_snippet` 是要贴的片段：

```html
<script src="$BASE/widget/v1/icomment.js?v=0.1.2" data-site-key="SITE_KEY" defer></script>
<ic-comment site-key="SITE_KEY" path="/posts/hello"></ic-comment>
<noscript>评论区需要 JavaScript</noscript>
```

要点：

- script 放 `<head>` 或 `</body>` 前均可，`defer` 必须有。
- 元素名是 `ic-comment`（`icomment` 不是合法自定义元素名）。
- `path` 写页面路径即可，会归一化成小写并去掉 query/hash；同一 path 的所有页面共用一个线程。
- 站点开 CSP 时给 script 补 `nonce`，并允许 `script-src`、`connect-src`、`style-src` 放行实例域名。

## 3. 验收

```bash
curl -sX POST $BASE/api/agent/sites/$SITE_ID/verify -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' -d '{"url":"https://blog.example.com/posts/hello"}'
```

三态：

- `{"status":"ok","found":[...]}` → 装完了，收工。
- `{"status":"partial","missing":[...],"hint":"…"}` → 按 `hint` 逐条改，再验一次。
- `{"status":"failed","reason":"timeout|unreachable|page_error|too_many_redirects","hint":"…"}` → 先让页面在浏览器里能打开。

服务端只抓公网地址、不执行页面 JS；回环与私有网段会被拒（实例开 `IC_VERIFY_PRIVATE=1` 才放行）。

## 4. 装完自检

```bash
curl -s "$BASE/api/site?path=/posts/hello" -H 'x-site-key: SITE_KEY'   # 200 + 站点配置
curl -sX POST $BASE/api/comments -H 'x-site-key: SITE_KEY' \
  -H 'content-type: application/json' \
  -d '{"path":"/posts/hello","body":"第一条","author":"装站自检"}'
```

返回 `status` 非 `deleted` 即写入成功；`pending` 表示等审核。

## 常见失败

| 现象 | 原因 |
|---|---|
| `challenge_required` | 站点开了人机验证，widget 会自动弹 Turnstile；纯脚本调用要先取 token |
| `rate_limited` + `Retry-After` | 同 IP 60s 内超过 3 条，等一会儿 |
| `site_not_found` | `site-key` 与站点对不上，或 `Origin`/`Referer` 缺失 |
| `validation_failed` | 看 `fields[]`，长度与格式都有上限（正文 8 KiB，链接过多会被判 spam） |
