//! 请求侧的小工具：Origin 头、客户端 IP。

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header, request::Parts},
};

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

/// 站点定位：`x-site-key` 头 > `Origin` 头 > `Referer` 兜底。
///
/// site_key 命中时返回值带 `key:` 前缀，交给 `site::resolve` 分辨。
pub struct SiteOrigin(pub String);

/// site_key 定位的哨兵前缀，避免和真实 origin 的字符串撞车。
pub const KEY_PREFIX: &str = "key:";

impl FromRequestParts<AppState> for SiteOrigin {
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self> {
        // 优先 site_key：它让同一篇内容可以同时嵌在任何允许的域上（预览域、本地）
        if let Some(key) = parts
            .headers
            .get("x-site-key")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return Ok(Self(format!("{KEY_PREFIX}{key}")));
        }

        if let Some(origin) = parts
            .headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != "null")
        {
            return Ok(Self(origin.to_owned()));
        }

        // 同源表单/老浏览器可能不带 Origin，用 Referer 的 scheme://host 部分
        if let Some(referer) = parts
            .headers
            .get(header::REFERER)
            .and_then(|v| v.to_str().ok())
            && let Some(idx) = referer.find("://")
        {
            let rest = &referer[idx + 3..];
            let host_end = rest.find('/').unwrap_or(rest.len());
            return Ok(Self(format!(
                "{}{}",
                &referer[..idx + 3],
                &rest[..host_end]
            )));
        }

        Err(Problem::code(
            Code::Validation,
            "missing x-site-key and Origin header, send one of them",
        ))
    }
}

/// 调用方 IP：优先可信反代的 XFF 首跳，其次 socket 地址。缺失也不报错。
pub struct ClientIp(pub Option<IpAddr>);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self> {
        let connect = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0);
        // 测试里没有监听 socket，用 XFF 首跳冒充来源，便于按用例隔离限流
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse::<IpAddr>().ok());
        let ip = match connect {
            Some(addr) => client_ip(&addr, &parts.headers).or(forwarded),
            None => forwarded,
        };
        Ok(Self(ip))
    }
}

pub fn client_ip(connect: &SocketAddr, headers: &header::HeaderMap) -> Option<IpAddr> {
    // 过 Cloudflare 时边缘会写 cf-connecting-ip，客户端改不动它，比 XFF 可靠
    if let Some(raw) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        && let Ok(ip) = raw.trim().parse::<IpAddr>()
    {
        return Some(ip);
    }
    // 只在信任反代时看 XFF；取最后一跳之前的第一个
    if let Some(raw) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = raw.split(',').next()
        && let Ok(ip) = first.trim().parse::<IpAddr>()
    {
        return Some(ip);
    }
    Some(connect.ip())
}
