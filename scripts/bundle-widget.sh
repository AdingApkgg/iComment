#!/usr/bin/env bash
# 把 widget 构建产物塞进 core 的静态资源里（单二进制分发，不依赖外部 CDN 也能跑）
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/packages/widget"
bun run build
cp dist/icomment.js "$root/apps/core/assets/icomment.js"
echo "bundled $(wc -c <"$root/apps/core/assets/icomment.js") bytes -> apps/core/assets/icomment.js"
