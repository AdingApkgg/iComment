# 部署：Cloudflare Tunnel（生产）

线上是 `https://icomment.saop.cc`，跑在这台机上的 compose 栈，前面由 Cloudflare Tunnel
收 HTTPS——本机的 80/443 归别的服务，这个站不碰。

| 路径    | 是什么                                                     |
| ------- | ---------------------------------------------------------- |
| `/`     | 落地页（讲清楚是什么、给 snippet、链到机器可读的说明）     |
| `/dash` | 控制台（登录后建站、看评论、审核、看审计留痕）             |
| `/api`  | 接口根（widget 公开读写 `/api`，agent 走 `/api/agent`）    |
| `/media/<hash>` | 附图，内容寻址，`immutable` 长缓存                 |

## 一次装好

```bash
# 1 环境变量：域名、库密码、会话密钥（openssl rand -hex 32）
cp deploy/.env.example deploy/.env
#   IC_DOMAIN=icomment.saop.cc
#   IC_TUNNEL_PORT=18080

# 2 起栈：叠 compose.tunnel.yml，Caddy 只听 127.0.0.1:18080，不抢 80/443
docker compose -f deploy/compose.yml -f deploy/compose.tunnel.yml \
  --env-file deploy/.env up -d --build
#   网差、容器里 cargo 拉不动 crates.io 时，改用宿主预编的镜像（见下一节）：
#   ./scripts/build-image.sh
#   docker compose -f deploy/compose.yml -f deploy/compose.tunnel.yml \
#     -f deploy/compose.prebuilt.yml --env-file deploy/.env up -d

# 3 隧道入口（本机常驻隧道 alice，见下）
cf tunnels config get 65ad5231-3e8f-4263-900e-dad5b9463dec > /tmp/tunnel.before.json
# 在 http_status:404 那条之前插一条：
#   {"hostname":"icomment.saop.cc","service":"http://127.0.0.1:18080"}
# 注意：CLI 的 update 会把整份 config 重写，还会往每条规则里塞 originRequest 默认值；
# 只想加一条时直接打 API 更稳（--tunnel-config-ingress @ingress.json 也行）：
TOKEN=$(jq -r .oauth_token ~/.config/cloudflare/config/default.json)
curl -sS -X PUT "https://api.cloudflare.com/client/v4/accounts/$CLOUDFLARE_ACCOUNT_ID/cfd_tunnel/65ad5231-3e8f-4263-900e-dad5b9463dec/configurations" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  --data '{"config":{"ingress":[...]}}'

# 4 DNS：CNAME 到隧道，proxied
cf dns records create -z saop.cc --body '{
  "type":"CNAME","name":"icomment",
  "content":"65ad5231-3e8f-4263-900e-dad5b9463dec.cfargotunnel.com",
  "proxied":true,"ttl":1}'

# 5 验
sleep 12 && curl -sS -o /dev/null -w '%{http_code}\n' https://icomment.saop.cc/
```

## 用宿主预编的二进制（慢网备选）

`scripts/build-image.sh` 在宿主 `cargo build --release` 一次，再把二进制和宿主根证书塞进
`deploy/Dockerfile.prebuilt` 打成本地镜像 `icomment-core:prebuilt`（约 60 MB），`up` 时
不再进容器编译。两个坑：

- **glibc 必须同代**：宿主是 Debian forky/sid（glibc 2.41），宿主编出来的二进制要
  `GLIBC_2.38`，distroless `cc-debian12` 只有 2.36，启动就报
  `version 'GLIBC_2.38' not found` 然后秒退。所以 prebuilt 的底是 `debian:trixie-slim`。
  换构建机或换底座时先确认这条。
- 容器内自建（`deploy/Dockerfile`）在 bookworm 里编译，仍然用 distroless，两条路互不影响。

## 为什么不用 Caddy 的自动 HTTPS

TLS 由 Cloudflare 边缘终止，回源是隧道内的明文 HTTP。所以 `compose.tunnel.yml` 把
Caddyfile 换成 `Caddyfile.tunnel`：`{$IC_DOMAIN}` 站点块换成 `:80`，其余（gzip、
安全头、`/widget/v1` 长缓存、`/api` `no-store`）原样保留，Caddy 只映射到
`127.0.0.1:${IC_TUNNEL_PORT}`，外面连不进来。

`IC_PUBLIC_BASE_URL` 仍是 `https://${IC_DOMAIN}`，所以 snippet、`/media` 绝对地址、
OAuth 回调都对；`cf-connecting-ip` 优先于 `X-Forwarded-For` 取来源 IP，限流不怕伪造。

## 改完发版

```bash
git pull && docker compose -f deploy/compose.yml -f deploy/compose.tunnel.yml \
  --env-file deploy/.env up -d --build core
```

镜像构建里 widget 由 bun 现场打包再进 Rust 二进制，所以只更新前端也要重建 core。

## 回滚

```bash
cf tunnels config update 65ad5231-3e8f-4263-900e-dad5b9463dec --body @/tmp/tunnel.before.json
cf dns records list --zone saop.cc    # 找到 icomment 那条的 id
cf dns records delete --zone saop.cc --id <id>
```
