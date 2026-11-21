#!/usr/bin/env bash
# 宿主编译 + 只打运行时镜像：机器上有 Rust 工具链时比容器内编译快得多
# （容器里 cargo 要从 crates.io 拉全量依赖，慢网下容易超时）。
# 产物镜像名默认为 icomment-core:prebuilt，配合 deploy/compose.prebuilt.yml 使用。
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

tag="${1:-icomment-core:prebuilt}"

./scripts/bundle-widget.sh
cargo build --release -p icomment-core

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cp target/release/icomment-core "$stage/"
docker build -f deploy/Dockerfile.prebuilt -t "$tag" "$stage"
echo "built $tag"
