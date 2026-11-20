//! 宿主站透传的已登录身份（`X-IC-Identity`）。
//!
//! payload/sig 都是 base64url 无填充：`payload.sig`。验签失败不拒绝评论，
//! 只是把这条标成 `identity_unverified`，按匿名处理。
//!
//! 这里只做「证明 site_secret 持有者签的」——`uid` 是站点自己的用户 id，
//! 只在站内有效，跨站不合并。

use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{
    domain::site,
    error::{Code, Problem, Result},
    state::AppState,
};

/// 允许的最长时间窗口（秒）。签名短时有效，过期就按匿名走。
pub const MAX_AGE_SECS: i64 = 300;
pub const HEADER: &str = "x-ic-identity";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payload {
    pub site: String,
    pub uid: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub site: String,
    pub uid: String,
    pub name: Option<String>,
    pub avatar: Option<String>,
}

fn sign(secret: &str, payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(payload.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// 给站点服务端用：签一段短时身份。
pub fn issue(secret: &str, site_key: &str, uid: &str, name: Option<&str>) -> String {
    let now = Utc::now().timestamp();
    let payload = Payload {
        site: site_key.to_owned(),
        uid: uid.to_owned(),
        name: name.map(str::to_owned),
        avatar: None,
        iat: now,
        exp: now + MAX_AGE_SECS,
    };
    let raw = serde_json::to_string(&payload).expect("payload serializes");
    let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());
    let sig = sign(secret, &encoded);
    format!("{encoded}.{sig}")
}

/// 取站点秘钥（只有 agent token / 控制台才会拿到）。
pub async fn site_secret(state: &AppState, site_id: uuid::Uuid) -> Result<String> {
    let secret: Option<String> =
        sqlx::query_scalar("select site_secret from site where id = $1 and deleted_at is null")
            .bind(site_id)
            .fetch_optional(state.db())
            .await?;
    secret.ok_or_else(|| Problem::code(Code::SiteNotFound, "site not found"))
}

/// 校验头。返回 `(identity, verified)`：验过为 true，带了但没验过为 false。
/// 完全没带头则返回 `None`。
pub fn read(site: &site::Site, headers: &HeaderMap) -> Option<(Identity, bool)> {
    let raw = headers.get(HEADER)?.to_str().ok()?.trim();
    let (encoded, sig) = raw.split_once('.')?;
    let Ok(bytes) = URL_SAFE_NO_PAD.decode(encoded) else {
        return Some((unverified(raw), false));
    };
    let Ok(payload) = serde_json::from_slice::<Payload>(&bytes) else {
        return Some((unverified(raw), false));
    };

    let mut mac = Hmac::<Sha256>::new_from_slice(site.site_secret.as_bytes()).expect("hmac key");
    mac.update(encoded.as_bytes());
    let Ok(expected) = URL_SAFE_NO_PAD.decode(sig) else {
        return Some((unverified(raw), false));
    };
    let now = Utc::now().timestamp();
    let ok = mac.verify_slice(&expected).is_ok()
        && payload.site == site.site_key
        && payload.exp >= now
        && payload.exp - payload.iat <= MAX_AGE_SECS;

    let identity = Identity {
        site: payload.site,
        uid: payload.uid,
        name: payload.name,
        avatar: payload.avatar,
    };
    Some((identity, ok))
}

/// 解析不出 payload 时的退化：连 uid 都不可信，只当匿名。
fn unverified(_raw: &str) -> Identity {
    Identity {
        site: String::new(),
        uid: String::new(),
        name: None,
        avatar: None,
    }
}
