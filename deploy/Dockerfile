# 单二进制镜像：内嵌 widget 与迁移，运行时只需要 PG + Redis。
# 构建上下文是仓库根目录。

FROM oven/bun:1.4.4-alpine AS widget
WORKDIR /src
COPY package.json bun.lock biome.json turbo.json tsconfig.base.json ./
COPY packages ./packages
RUN bun install --frozen-lockfile
RUN cd packages/widget && bun run build && cp dist/icomment.js /tmp/icomment.js

FROM rust:1.94-bookworm AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY apps ./apps
COPY --from=widget /tmp/icomment.js ./apps/core/assets/icomment.js
RUN cargo build --release -p icomment-core

FROM gcr.io/distroless/cc-debian12:nonroot
# distroless 自带 ca-certificates、非 root 用户；没有 shell 也没有包管理器。
COPY --from=build /src/target/release/icomment-core /usr/local/bin/icomment-core

WORKDIR /data
ENV IC_PROFILE=prod IC_BIND=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/icomment-core"]
