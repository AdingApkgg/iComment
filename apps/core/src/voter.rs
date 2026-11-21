//! 谁在投票：能有身份就用身份，没有就发一个匿名投票 id cookie。
//!
//! cookie 里只有随机数，不代表账号；换个浏览器就是另一个人，这正好符合需求。

use axum::http::{HeaderMap, header};
use uuid::Uuid;

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

pub const COOKIE: &str = "ic_voter";

/// 解析 `ic_voter`，格式 `id.sig`；签名错就当没有。
fn from_cookie(headers: &HeaderMap, state: &AppState) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        let raw = value.to_str().ok()?;
        for pair in raw.split(';') {
            let pair = pair.trim();
            if let Some(rest) = pair.strip_prefix("ic_voter=") {
                let (id, sig) = rest.split_once('.')?;
                if id.len() == 32
                    && Uuid::try_parse(id).is_ok()
                    && crate::web::sign_plain(state, id) == sig
                {
                    return Some(id.to_owned());
                }
            }
        }
    }
    None
}

/// 返回 (投票人, 需要新发的 cookie)。
pub fn resolve(headers: &HeaderMap, state: &AppState) -> (String, Option<String>) {
    if let Some(id) = from_cookie(headers, state) {
        return (id, None);
    }
    let id = Uuid::now_v7().simple().to_string();
    let cookie = format!(
        "{COOKIE}={id}.{}; Path=/; Max-Age=31536000; SameSite=Lax",
        crate::web::sign_plain(state, &id)
    );
    (id, Some(cookie))
}

/// 已有身份（透传 uid）时优先用身份，否则落回 cookie。
pub fn identity_or_cookie(
    headers: &HeaderMap,
    state: &AppState,
    identity: Option<&str>,
) -> (String, Option<String>) {
    match identity {
        Some(uid) if !uid.is_empty() => (format!("u:{uid}"), None),
        _ => resolve(headers, state),
    }
}

pub fn missing() -> Problem {
    Problem::code(Code::Validation, "voter id required")
}

/// 校验外部传入的投票人 id 长度。
pub fn clean(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 80 {
        return Err(missing());
    }
    Ok(raw.to_owned())
}
