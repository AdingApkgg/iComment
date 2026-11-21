//! 反垃圾三件套：限流、Turnstile 校验、内容启发式。
//!
//! 全部失败都映射成带行动指引的 problem，让 agent 能自己修。

use std::{net::IpAddr, sync::LazyLock};

use chrono::Utc;
use redis::AsyncCommands;
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

pub const COMMENT_BURST: i64 = 3; // 每 60s
pub const COMMENT_HOURLY: i64 = 24; // 每 3600s
pub const UPLOAD_HOURLY: i64 = 40; // 每 3600s，附图比评论宽松一点
pub const REACTION_BURST: i64 = 30; // 每 60s，比发评论宽松得多

/// 反应投票的限流键：站点 + IP。
pub fn reaction_key(site: uuid::Uuid, ip: Option<std::net::IpAddr>) -> String {
    match ip {
        Some(ip) => format!("ic:rl:r:{site}:{ip}"),
        None => format!("ic:rl:r:{site}:none"),
    }
}

fn digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\x1f");
    }
    format!("{:x}", hasher.finalize())[..32].to_owned()
}

/// IP 哈希带每日盐，跨天无法关联。
pub async fn ip_hash(state: &AppState, ip: Option<IpAddr>) -> Option<String> {
    let ip = ip?;
    let mut conn = state.redis().clone();
    let salt: Option<String> = conn
        .get(format!("ic:salt:{}", Utc::now().format("%Y%m%d")))
        .await
        .ok()
        .flatten();
    let salt = match salt {
        Some(s) => s,
        None => {
            let new = digest(&[&Utc::now().timestamp().to_string()]);
            let _: std::result::Result<(), _> = conn
                .set_ex(
                    format!("ic:salt:{}", Utc::now().format("%Y%m%d")),
                    &new,
                    172_800,
                )
                .await;
            new
        }
    };
    Some(digest(&[&salt, &ip.to_string()])[..16].to_owned())
}

/// 固定窗口计数。超限返回带剩余秒数的 429。
pub async fn hit(state: &AppState, key: &str, limit: i64, window_secs: usize) -> Result<i64> {
    let mut conn = state.redis().clone();
    let count: i64 = conn.incr(key, 1).await?;
    if count == 1 {
        let _: std::result::Result<(), _> = conn.expire(key, window_secs as i64).await;
    }
    if count > limit {
        return Err(Problem::code(
            Code::RateLimited,
            format!("{count} over limit {limit} in {window_secs}s"),
        ));
    }
    Ok(count)
}

/// 单站日写入软顶：超了只强制进队列，不拒。
pub const DAILY_SOFT_CAP: i64 = 1000;

/// 只计数不报错：返回本次累计值，由调用方决定怎么降级。
pub async fn count(state: &AppState, key: &str, window_secs: usize) -> anyhow::Result<i64> {
    let mut conn = state.redis().clone();
    let count: i64 = conn.incr(key, 1).await?;
    if count == 1 {
        let _: std::result::Result<(), _> = conn.expire(key, window_secs as i64).await;
    }
    Ok(count)
}

/// Turnstile 校验。未配置 secret（dev）时直接通过。
/// 三态策略：always 一律验；suspicious 只在内容可疑时验；off 不验。
/// 已经被判成 spam 的直接进垃圾箱，不必再耗一次验证。
pub fn challenge_needed(mode: &str, spam: bool) -> bool {
    !spam && matches!(mode, "always" | "suspicious")
}

pub async fn verify_challenge(
    state: &AppState,
    token: Option<&str>,
    ip: Option<IpAddr>,
) -> Result<()> {
    let Some(secret) = state.cfg().turnstile_secret.as_deref() else {
        return Ok(());
    };
    let token = token
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Problem::code(Code::ChallengeRequired, "challenge_token missing"))?;

    let mut form = vec![("secret", secret), ("response", token)];
    let ip_string;
    if let Some(ip) = ip {
        ip_string = ip.to_string();
        form.push(("remoteip", &ip_string));
    }

    let res = state
        .http()
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .form(&form)
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await;

    let payload: serde_json::Value = match res {
        Ok(resp) => resp.json().await.unwrap_or(serde_json::Value::Null),
        Err(err) => {
            tracing::warn!(%err, "turnstile unreachable");
            // 上游抖动不该挡住评论
            return Ok(());
        }
    };

    if payload.get("success").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(());
    }
    let codes = payload
        .get("error-codes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    Err(Problem::code(
        Code::ChallengeFailed,
        format!(
            "turnstile rejected: {}",
            if codes.is_empty() { "unknown" } else { &codes }
        ),
    ))
}

static HOST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://(?:[^\s)>"']*@)?([^/\s)>"'?#]+)"#).unwrap());

/// 正文里出现过的域名，已小写、去端口。
pub fn link_hosts(body_md: &str) -> Vec<String> {
    HOST.captures_iter(body_md)
        .filter_map(|cap| cap.get(1))
        .map(|host| {
            host.as_str()
                .rsplit_once(':')
                .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
                .map_or_else(|| host.as_str().to_owned(), |(h, _)| h.to_owned())
                .trim_end_matches('.')
                .to_lowercase()
        })
        .collect()
}

/// 命中的黑名单域名。`example.com` 同时管住 `cdn.example.com`。
pub fn blocked_hit(hosts: &[String], blocked: &[String]) -> Option<String> {
    hosts
        .iter()
        .find(|host| {
            blocked
                .iter()
                .any(|b| host.as_str() == b || host.ends_with(&format!(".{b}")))
        })
        .cloned()
}

/// 内容启发式。返回 (是否 spam, 是否需人工)。
pub fn sniff(body_md: &str, author_name: Option<&str>, blocked: &[String]) -> (bool, bool) {
    let hosts = link_hosts(body_md);
    let links = hosts.len();
    let lower = body_md.to_lowercase();

    let spam_phrases = [
        "casino",
        "slot gacor",
        "buy followers",
        "seo services",
        "backlink",
    ];
    let phrase_hits = spam_phrases.iter().filter(|p| lower.contains(*p)).count();

    let spam =
        blocked_hit(&hosts, blocked).is_some() || links >= 4 || (links >= 2 && phrase_hits >= 1);
    // 只对「短且没昵称」这种两头都弱的意见，才值得人工看一眼
    let unnamed = author_name.map(str::trim).unwrap_or("").is_empty();
    let needs_review = unnamed && body_md.chars().count() < 12;
    (spam, needs_review)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_heavy_is_spam() {
        let (spam, _) = sniff(
            "a https://a b https://b c https://c d https://d",
            Some("x"),
            &[],
        );
        assert!(spam);
    }

    #[test]
    fn plain_comment_is_clean() {
        let (spam, review) = sniff("这个主题很不错，收藏了", Some("小明"), &[]);
        assert!(!spam);
        assert!(!review);
    }

    #[test]
    fn blocked_domain_is_spam() {
        let (spam, _) = sniff(
            "看 https://cdn.seo-spam.example 和 https://ok.example",
            Some("x"),
            &["seo-spam.example".to_owned()],
        );
        assert!(spam);
    }

    #[test]
    fn blocked_domain_matches_subdomain_only() {
        let hosts = link_hosts("a https://cdn.spam.example/x b https://notspam.example");
        assert_eq!(
            blocked_hit(&hosts, &["spam.example".to_owned()]),
            Some("cdn.spam.example".to_owned())
        );
    }

    #[test]
    fn short_without_name_needs_review() {
        let (_, review) = sniff("+1", None, &[]);
        assert!(review);
    }
}
