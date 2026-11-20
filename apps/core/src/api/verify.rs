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
        return Ok(VerifyResult {
            url,
            http_status: None,
            verdict: Verdict::Failed {
                reason: "private_host",
                hint: "本机和内网地址服务端抓不到：先部署到公网，或给服务端加 IC_VERIFY_PRIVATE=1"
                    .to_owned(),
            },
        });
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

    let found = scan(&body, site_key);
    let missing = found.missing.clone();
    let verdict = if missing.is_empty() {
        Verdict::Ok {
            found: found.ok.clone(),
        }
    } else {
        Verdict::Partial {
            hint: build_hint(&found, site_key, &state.cfg().widget_cdn),
            missing,
        }
    };

    Ok(VerifyResult {
        url,
        http_status: Some(status),
        verdict,
    })
}

/// 页面结构体检。只看标签形状，不认 JS 生成的东西。
#[derive(Debug, Default)]
struct Scan {
    /// 判定为通过的项目（与 `missing` 互补）
    ok: Vec<&'static str>,
    missing: Vec<&'static str>,
    /// 页面上出现的、和我们期望不一致的 site-key
    other_keys: Vec<String>,
    script_url: Option<String>,
    /// `<ic-comment>` 出现在 `</body>` 之后
    element_after_body: bool,
}

fn scan(body: &str, site_key: &str) -> Scan {
    let lower = body.to_ascii_lowercase();
    let key = site_key.to_ascii_lowercase();
    let mut out = Scan::default();

    let scripts: Vec<&str> = tags(&lower, "<script").collect();
    let elements: Vec<&str> = tags(&lower, "<ic-comment")
        .chain(tags(&lower, "<icomment"))
        .collect();

    // 认脚本：优先带 data-site-key 的那个，退而求其次是 src 里有 icomment.js 的
    let chosen = scripts
        .iter()
        .find(|tag| key_of_script(tag).is_some())
        .or_else(|| scripts.iter().find(|tag| tag.contains("icomment.js")))
        .copied();
    out.script_url = chosen.and_then(src_of).map(str::to_owned);
    let script_ok = chosen.and_then(src_of).is_some_and(|src| {
        src.split('?')
            .next()
            .unwrap_or(src)
            .ends_with("icomment.js")
    });
    if script_ok {
        out.ok.push("script");
    } else {
        out.missing.push("script");
    }
    if chosen.is_some_and(|tag| !tag.contains("defer")) {
        out.missing.push("defer");
    }

    // 元素：出现即认，位置不对单独提示
    let element = &elements[0..0];
    let _ = element;
    if elements.is_empty() {
        out.missing.push("element");
    } else {
        out.ok.push("element");
        let first = lower
            .find("<ic-comment")
            .or_else(|| lower.find("<icomment"))
            .unwrap_or(0);
        out.element_after_body = lower.rfind("</body>").is_some_and(|end| first > end);
        if elements.iter().any(|tag| tag.contains("path=")) {
            out.ok.push("path");
        } else {
            out.missing.push("path");
        }
    }

    // key 挂在元素或脚本上任意一处都算；页面上别的 key 记下来
    let attr = format!("site-key=\"{key}\"");
    let key_ok = elements.iter().any(|tag| tag.contains(&attr))
        || chosen.is_some_and(|tag| tag.contains(&format!("data-{attr}")));
    if key_ok {
        out.ok.push("site_key");
    } else {
        out.missing.push("site_key");
    }
    for tag in scripts.iter().chain(elements.iter()) {
        if let Some(value) = key_of_script(tag).or_else(|| key_of_element(tag))
            && value != key
            && !out.other_keys.iter().any(|k| k == value)
        {
            out.other_keys.push(value.to_owned());
        }
    }

    out
}

fn attr_of<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let after = tag.split(name).nth(1)?;
    let after = after.strip_prefix("=\"")?;
    after.split('"').next().filter(|v| !v.is_empty())
}

fn key_of_script(tag: &str) -> Option<&str> {
    attr_of(tag, "data-site-key")
}

fn key_of_element(tag: &str) -> Option<&str> {
    attr_of(tag, "site-key")
}

fn tags<'a>(haystack: &'a str, prefix: &'static str) -> impl Iterator<Item = &'a str> + 'a {
    haystack
        .match_indices(prefix)
        .map(move |(at, _)| &haystack[at..])
        .map(move |rest| match rest.find('>') {
            Some(end) => &rest[..end],
            None => rest,
        })
}

fn src_of(tag: &str) -> Option<&str> {
    let after = tag.split("src=\"").nth(1)?;
    after.split('"').next()
}

/// 只要 src 最后一段是 icomment.js，CDN 换到哪都算对。
fn script_url_is_ours(tag: &str) -> bool {
    src_of(tag).is_some_and(|src| {
        src.split('?')
            .next()
            .unwrap_or(src)
            .ends_with("icomment.js")
    })
}

fn build_hint(found: &Scan, site_key: &str, cdn: &str) -> String {
    let snippet = format!(
        "<script src=\"{cdn}/icomment.js?v={}\" data-site-key=\"{site_key}\" defer></script>",
        env!("CARGO_PKG_VERSION")
    );
    let mut parts = Vec::new();

    if found.missing.contains(&"script") {
        match &found.script_url {
            Some(src) => parts.push(format!(
                "脚本 src 是 {src}，改成 {cdn}/icomment.js（或直接用整段：{snippet}）"
            )),
            None => parts.push(format!("把这段放进 </head> 前：{snippet}")),
        }
    }
    if found.missing.contains(&"site_key") && found.missing.contains(&"script") {
        // snippet 里已经带了 key，不必再单独说
    } else if found.missing.contains(&"site_key") {
        parts.push(format!("脚本标签上补 data-site-key=\"{site_key}\""));
    }
    if found.missing.contains(&"defer") {
        parts.push("脚本加 defer，否则阻塞首屏".to_owned());
    }
    if found.missing.contains(&"element") {
        parts.push(format!(
            "在文章正文末尾放 <ic-comment site-key=\"{site_key}\" api-base=\"…\" path=\"…\"></ic-comment>"
        ));
    } else if found.element_after_body {
        parts.push("把 <ic-comment> 挪到 </body> 里面".to_owned());
    }
    if found.missing.contains(&"path") {
        parts.push("元素上补 path，通常是当前页面的 URL 路径".to_owned());
    }
    if found.ok.contains(&"element") && found.missing.contains(&"site_key") {
        parts.push(format!("元素上也补 site-key=\"{site_key}\""));
    }

    let wrong: Vec<_> = found
        .other_keys
        .iter()
        .filter(|k| k.as_str() != site_key)
        .collect();
    if !wrong.is_empty() {
        parts.push(format!(
            "页面上出现的是 {}，和本站 {site_key} 对不上",
            wrong
                .iter()
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join("、")
        ));
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

#[cfg(test)]
mod scan_tests {
    use super::scan;

    const KEY: &str = "abc123";

    #[test]
    fn clean_page_reports_nothing_missing() {
        let page = format!(
            "<html><body><ic-comment site-key=\"{KEY}\" path=\"/p\"></ic-comment>\
             <script src=\"https://cdn/icomment.js\" data-site-key=\"{KEY}\" defer></script></body></html>"
        );
        let out = scan(&page, KEY);
        assert!(out.missing.is_empty(), "{:?}", out.missing);
        assert_eq!(out.ok.len(), 4);
    }

    #[test]
    fn wrong_cdn_reports_the_actual_url() {
        let page = format!(
            "<script src=\"https://cdn/js/widget.js\" data-site-key=\"{KEY}\" defer></script>\
             <ic-comment site-key=\"{KEY}\" path=\"/p\"></ic-comment>"
        );
        let out = scan(&page, KEY);
        assert_eq!(out.missing, ["script"]);
        assert_eq!(out.script_url.as_deref(), Some("https://cdn/js/widget.js"));
    }

    #[test]
    fn foreign_key_is_called_out() {
        let page = "<script src=\"/icomment.js\" data-site-key=\"zzz\" defer></script>\
                    <ic-comment site-key=\"zzz\" path=\"/p\"></ic-comment>";
        let out = scan(page, KEY);
        assert!(out.missing.contains(&"site_key"));
        assert!(out.other_keys.contains(&"zzz".to_owned()));
    }

    #[test]
    fn element_after_body_is_flagged() {
        let page = format!(
            "<html><body><script src=\"a/icomment.js\" data-site-key=\"{KEY}\" defer></script></body>\
             <ic-comment path=\"/p\"></ic-comment></html>"
        );
        assert!(scan(&page, KEY).element_after_body);
    }
}
