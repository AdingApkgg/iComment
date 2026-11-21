//! Akismet 兜底：本地规则放过的、有嫌疑的再问一次外部服务。
//!
//! 只有 `IC_AKISMET_KEY` 配了才会真的发请求；超时或报错一律退回本地判断，
//! 不让外部服务的抖动把评论卡住。

use std::time::Duration;

use serde_json::json;

use crate::state::AppState;

const TIMEOUT: Duration = Duration::from_secs(3);

/// 组装请求体；`site` 是站点 origin，Akismet 用它做站点识别。
pub fn payload<'a>(
    site_origin: &'a str,
    ip: Option<&'a str>,
    ua: Option<&'a str>,
    body_md: &'a str,
    author_name: Option<&'a str>,
    author_email: Option<&'a str>,
) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    fields.insert("blog".into(), json!(site_origin));
    fields.insert("comment_type".into(), json!("comment"));
    fields.insert("comment_content".into(), json!(body_md));
    if let Some(ip) = ip {
        fields.insert("user_ip".into(), json!(ip));
    }
    if let Some(ua) = ua {
        fields.insert("user_agent".into(), json!(ua));
    }
    if let Some(name) = author_name {
        fields.insert("comment_author".into(), json!(name));
    }
    if let Some(email) = author_email {
        fields.insert("comment_author_email".into(), json!(email));
    }
    serde_json::Value::Object(fields)
}

/// `true` 表示判定为垃圾；`None` 表示没配或这次没问成。
pub async fn says_spam(
    state: &AppState,
    site_origin: &str,
    ip: Option<&str>,
    ua: Option<&str>,
    body_md: &str,
    author_name: Option<&str>,
    author_email: Option<&str>,
) -> Option<bool> {
    let key = state.cfg().akismet_key.as_deref()?;
    let body = payload(site_origin, ip, ua, body_md, author_name, author_email);

    let resp = state
        .http()
        .post(&state.cfg().akismet_endpoint)
        .timeout(TIMEOUT)
        .query(&[("key", key)])
        .json(&body)
        .send()
        .await;

    match resp {
        Ok(resp) => match resp.text().await {
            Ok(text) => parse(&text),
            Err(e) => {
                tracing::warn!(error = %e, "akismet: unreadable body");
                None
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "akismet: request failed");
            None
        }
    }
}

/// 只有明确的 `true` 才当垃圾；别的一律"没意见"。
fn parse(text: &str) -> Option<bool> {
    match text.trim() {
        "true" => Some(true),
        "false" => Some(false),
        other => {
            tracing::warn!(reply = %other, "akismet: unexpected reply");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_true_is_spam() {
        assert_eq!(parse("true"), Some(true));
        assert_eq!(parse(" false\n"), Some(false));
        assert_eq!(parse(""), None);
        assert_eq!(parse("Invalid API key"), None);
    }

    #[test]
    fn payload_skips_absent_fields() {
        let body = payload("https://a.example", None, Some("UA"), "hi", None, None);
        assert_eq!(body["blog"], "https://a.example");
        assert_eq!(body["user_agent"], "UA");
        assert_eq!(body["comment_type"], "comment");
        assert!(body.get("user_ip").is_none());
        assert!(body.get("comment_author").is_none());
    }
}
