//! 安装验收：服务端自己去抓页面，判断 widget 是否真的挂上了。
//!
//! 只能抓公网地址，禁回环与链路本地段；跟随最多 3 跳，超时 8s。

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::Agent,
    domain::site,
    error::{Code, Problem, Result},
    ratelimit::{self, VERIFY_PER_SITE},
    state::AppState,
};

pub const UA: &str = "icomment-verify/1";
const TIMEOUT_SECS: u64 = 8;
const MAX_HOPS: usize = 3;

#[derive(Debug, Deserialize)]
pub struct VerifyInput {
    pub url: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Verdict {
    Ok {
        found: Vec<&'static str>,
    },
    Partial {
        missing: Vec<&'static str>,
        hint: String,
    },
    Failed {
        reason: &'static str,
        hint: String,
    },
}

#[derive(Debug, Serialize)]
pub struct VerifyResult {
    #[serde(flatten)]
    pub verdict: Verdict,
    pub url: String,
    pub http_status: Option<u16>,
}

/// 抓一页并判定。判定只看 HTML 结构，不执行页面 JS。
pub async fn verify_install(
    State(state): State<AppState>,
    agent: Agent,
    Path(site_id): Path<Uuid>,
    Json(input): Json<VerifyInput>,
) -> Result<Json<VerifyResult>> {
    if !agent.allows_site(site_id) {
        return Err(Problem::code(
            Code::Forbidden,
            "token bound to another site",
        ));
    }
    let site = site::by_id(&state, site_id).await?;
    Ok(Json(run(&state, &site.site_key, &input.url).await?))
}

/// 控制台与 API 共用，`url` 是用户输入原文。
pub async fn run(state: &AppState, site_key: &str, url: &str) -> Result<VerifyResult> {
    let url = url.trim().to_owned();
    let parsed = match reqwest::Url::parse(&url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => u,
        _ => return Err(Problem::field("url", "must be an absolute http(s) url")),
    };
    if let Some(host) = parsed.host_str()
        && is_private(host)
        && !state.cfg().verify_private
    {
        return Err(Problem::field("url", "private/local hosts are not fetched"));
    }
    ratelimit::charge(state, site_key, VERIFY_PER_SITE).await?;

    let (status, body) = match fetch(state, &parsed).await {
        Ok(pair) => pair,
        Err(reason) => {
            return Ok(VerifyResult {
                url,
                http_status: None,
                verdict: Verdict::Failed {
                    reason,
                    hint: "确认地址能在浏览器里打开；本地和 127.0.0.1 抓不到".to_owned(),
                },
            });
        }
    };

    if status >= 400 {
        return Ok(VerifyResult {
            url,
            http_status: Some(status),
            verdict: Verdict::Failed {
                reason: "page_error",
                hint: format!("页面返回 {status}，先让页面 200 再验收"),
            },
        });
    }

    let lower = body.to_ascii_lowercase();
    let has_script = lower.contains("/widget/v1/icomment.js");
    let has_element = lower.contains("<ic-comment") || lower.contains("<icomment");
    let key = site_key.to_ascii_lowercase();
    let has_key = lower.contains(&key);
    let has_path = lower.contains("path=");

    let mut missing = Vec::new();
    if !has_script {
        missing.push("script");
    }
    if !has_element {
        missing.push("element");
    }
    if !has_key {
        missing.push("site_key");
    }
    if !has_path {
        missing.push("path");
    }

    let verdict = if missing.is_empty() {
        Verdict::Ok {
            found: vec!["script", "element", "site_key", "path"],
        }
    } else {
        Verdict::Partial {
            hint: build_hint(&missing, site_key),
            missing,
        }
    };

    Ok(VerifyResult {
        url,
        http_status: Some(status),
        verdict,
    })
}

fn build_hint(missing: &[&str], site_key: &str) -> String {
    let mut parts = Vec::new();
    if missing.contains(&"script") {
        parts.push(format!(
            "把 <script src=\"${{IC_WIDGET_CDN}}/icomment.js\" data-site-key=\"{site_key}\" defer></script> 放进 <head>"
        ));
    }
    if missing.contains(&"element") {
        parts.push("在文章正文末尾放 <ic-comment site-key=… path=…></ic-comment>".to_owned());
    }
    if missing.contains(&"site_key") {
        parts.push(format!("元素上补 site-key=\"{site_key}\""));
    }
    if missing.contains(&"path") {
        parts.push("元素上补 path，通常是当前页面的 URL 路径".to_owned());
    }
    parts.join("；")
}

async fn fetch(
    state: &AppState,
    url: &reqwest::Url,
) -> std::result::Result<(u16, String), &'static str> {
    let mut current = url.clone();
    for _ in 0..MAX_HOPS {
        let resp = state
            .http()
            .get(current.clone())
            .header(reqwest::header::USER_AGENT, UA)
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    "timeout"
                } else {
                    "unreachable"
                }
            })?;

        let status = resp.status().as_u16();
        if (300..400).contains(&status)
            && let Some(next) = resp.headers().get(reqwest::header::LOCATION)
        {
            let text = next.to_str().map_err(|_| "bad_redirect")?;
            current = current.join(text).map_err(|_| "bad_redirect")?;
            continue;
        }
        let body = resp.text().await.map_err(|_| "read_failed")?;
        return Ok((status, body));
    }
    Err("too_many_redirects")
}

/// 回环、私有网段、链路本地、`.local` 都当作抓不到。
fn is_private(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_unique_local()
                    || v6.is_unicast_link_local()
            }
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_private;

    #[test]
    fn rejects_local_targets() {
        for host in [
            "localhost",
            "127.0.0.1",
            "10.0.0.2",
            "192.168.1.4",
            "::1",
            "box.local",
        ] {
            assert!(is_private(host), "{host} should be private");
        }
        for host in ["example.com", "8.8.8.8", "blog.saop.cc"] {
            assert!(!is_private(host), "{host} should be public");
        }
    }
}
