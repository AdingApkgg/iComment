//! 域名归属校验：IndexNow 那套——把 `<key>.txt` 放到域名根下，内容就是 key，
//! 服务端回抓比对通过才允许建站。
//!
//! 目的很具体：防止有人抢先把别人的域名注册成自己的站点、拿到 site_key，
//! 等站主装 widget 时把评论截到自己后台。

use chrono::{DateTime, Duration, Utc};
use reqwest::Url;
use uuid::Uuid;

use crate::{
    api::verify,
    domain::site::{self, Site},
    error::{Code, Problem, Result},
    normalize::normalize_origin,
    ratelimit::{self, CLAIM_PER_ORIGIN},
    state::AppState,
};

/// 申请 24 小时内有效，够放文件、跑一次部署。
const TTL_HOURS: i64 = 24;
/// 同一个申请最多试 10 次，防止拿它当探测跳板。
const MAX_ATTEMPTS: i32 = 10;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Claim {
    pub id: Uuid,
    pub account_id: Uuid,
    pub origin: String,
    pub claim_key: String,
    pub attempts: i32,
    pub verified_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
}

impl Claim {
    /// 要放到域名根下的那个文件地址。
    pub fn file_url(&self) -> String {
        file_url(&self.origin, &self.claim_key)
    }

    pub fn expired(&self) -> bool {
        self.expires_at <= Utc::now()
    }
}

const COLS: &str = "id, account_id, origin, claim_key, attempts, verified_at, expires_at";

/// 32 位十六进制，满足 IndexNow 对 key 的字符要求（字母数字）。
pub fn new_key() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// `https://example.com/<key>.txt`
pub fn file_url(origin: &str, key: &str) -> String {
    let host = origin
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    format!("https://{host}/{key}.txt")
}

fn host_of(origin: &str) -> Option<String> {
    Url::parse(origin)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
}

/// 开一个申请：域名已归属别人时直接拒，自己已有待验证的申请就复用。
pub async fn start(state: &AppState, account_id: Uuid, raw_origin: &str) -> Result<Claim> {
    let origin = normalize_origin(raw_origin)
        .ok_or_else(|| Problem::field("origin", "not a usable origin"))?;

    let taken: Option<Uuid> = sqlx::query_scalar(
        "select id from site where lower(origin) = lower($1) and deleted_at is null",
    )
    .bind(&origin)
    .fetch_optional(state.db())
    .await?;
    if taken.is_some() {
        return Err(Problem::field(
            "origin",
            "already registered; if it is yours, sign in with the account that owns it",
        ));
    }

    // 清掉过期的待验证申请，免得同一个域名堆一摞
    sqlx::query("delete from site_claim where account_id = $1 and verified_at is null and expires_at <= now()")
        .bind(account_id)
        .execute(state.db())
        .await?;

    let existing: Option<Claim> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from site_claim \
         where account_id = $1 and lower(origin) = lower($2) and verified_at is null \
           and expires_at > now() \
         order by created_at desc limit 1"
    )))
    .bind(account_id)
    .bind(&origin)
    .fetch_optional(state.db())
    .await?;
    if let Some(claim) = existing {
        return Ok(claim);
    }

    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "insert into site_claim (id, account_id, origin, claim_key, expires_at) \
         values ($1, $2, $3, $4, $5) returning {COLS}"
    )))
    .bind(Uuid::now_v7())
    .bind(account_id)
    .bind(&origin)
    .bind(new_key())
    .bind(Utc::now() + Duration::hours(TTL_HOURS))
    .fetch_one(state.db())
    .await
    .map_err(|err| match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            Problem::field("origin", "another claim for this origin is already pending")
        }
        _ => err.into(),
    })
}

pub async fn by_id(state: &AppState, account_id: Uuid, claim_id: Uuid) -> Result<Claim> {
    let claim: Option<Claim> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from site_claim where id = $1 and account_id = $2"
    )))
    .bind(claim_id)
    .bind(account_id)
    .fetch_optional(state.db())
    .await?;
    claim.ok_or_else(|| Problem::code(Code::NotFound, "claim not found"))
}

/// 按 origin + key 找申请，给 agent 用（它拿不到 claim id 也照样能验）。
pub async fn by_key(
    state: &AppState,
    account_id: Uuid,
    raw_origin: &str,
    key: &str,
) -> Result<Claim> {
    let origin = normalize_origin(raw_origin)
        .ok_or_else(|| Problem::field("origin", "not a usable origin"))?;
    let claim: Option<Claim> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from site_claim \
         where account_id = $1 and lower(origin) = lower($2) and claim_key = $3 and verified_at is null"
    )))
    .bind(account_id)
    .bind(&origin)
    .bind(key.trim())
    .fetch_optional(state.db())
    .await?;
    claim.ok_or_else(|| {
        Problem::field(
            "claim_key",
            "no pending claim for this origin; POST /api/v1/agent/sites/claim first",
        )
    })
}

/// 校验并建站。成功返回 (申请, 站点)。
///
/// `input.origin` 被忽略——以申请里验证过的域名为准，防止中途换域。
pub async fn verify(
    state: &AppState,
    account_id: Uuid,
    claim: Claim,
    input: site::CreateSite,
) -> Result<(Claim, Site)> {
    if claim.verified_at.is_some() {
        return Err(Problem::field("claim", "already used"));
    }
    if claim.expired() {
        return Err(Problem::field("claim", "expired; start a new claim"));
    }
    if claim.attempts >= MAX_ATTEMPTS {
        return Err(Problem::code(
            Code::RateLimited,
            "too many attempts for this claim; start a new one",
        ));
    }

    ratelimit::charge(state, &claim.origin, CLAIM_PER_ORIGIN).await?;

    if let Err(problem) = probe(state, &claim).await {
        sqlx::query("update site_claim set attempts = attempts + 1 where id = $1")
            .bind(claim.id)
            .execute(state.db())
            .await?;
        return Err(problem);
    }

    // 先抢占这条申请（并发点击时只有一次能成），再建站
    let claimed: Option<Uuid> = sqlx::query_scalar(
        "update site_claim set verified_at = now() \
         where id = $1 and verified_at is null and expires_at > now() returning id",
    )
    .bind(claim.id)
    .fetch_optional(state.db())
    .await?;
    if claimed.is_none() {
        return Err(Problem::field("claim", "already used or expired"));
    }

    let created = site::create(
        state,
        account_id,
        site::CreateSite {
            origin: claim.origin.clone(),
            ..input
        },
    )
    .await;
    match created {
        Ok(site) => Ok((claim, site)),
        Err(err) => {
            // 建站失败就把申请放回去，别让人白等 24 小时
            sqlx::query("update site_claim set verified_at = null where id = $1")
                .bind(claim.id)
                .execute(state.db())
                .await?;
            Err(err)
        }
    }
}

/// 回抓 `<key>.txt` 并比对。校验关掉时（仅非 prod）直接放行。
async fn probe(state: &AppState, claim: &Claim) -> Result<()> {
    if !state.cfg().claim_verify {
        return Ok(());
    }

    let url = Url::parse(&claim.file_url())
        .map_err(|err| Problem::code(Code::Internal, err.to_string()))?;
    let host =
        host_of(&claim.origin).ok_or_else(|| Problem::field("origin", "not a usable origin"))?;
    if verify::is_private(&host) && !state.cfg().verify_private {
        return Err(Problem::field(
            "origin",
            "本机/内网域名服务端抓不到：先部署到公网，或给服务端加 IC_VERIFY_PRIVATE=1",
        ));
    }

    let (status, body) = verify::fetch(state, &url).await.map_err(|reason| {
        Problem::field(
            "origin",
            format!(
                "抓不到 {}（{reason}）；确认文件已发布、能匿名访问",
                claim.file_url()
            ),
        )
    })?;

    check_body(status, &body, &claim.claim_key)
        .map_err(|reason| Problem::field("origin", format!("{}：{reason}", claim.file_url())))
}

/// 只看回包对不对：200 + 内容就是 key（允许首尾空白/换行）。
fn check_body(status: u16, body: &str, key: &str) -> std::result::Result<(), String> {
    if status != 200 {
        return Err(format!("返回 {status}，文件要放在域名根目录且不需要登录"));
    }
    if body.trim() != key {
        return Err(format!("内容不是这个 key，文件里只放一行 {key}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_32_hex_and_unique() {
        let a = new_key();
        let b = new_key();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn body_must_be_exactly_the_key() {
        assert!(check_body(200, "abc123", "abc123").is_ok());
        // 编辑器加的尾换行不算错
        assert!(check_body(200, "abc123\n", "abc123").is_ok());
        assert!(check_body(200, "  abc123  \n", "abc123").is_ok());
        assert!(check_body(200, "<html>404</html>", "abc123").is_err());
        assert!(check_body(404, "", "abc123").is_err());
        assert!(check_body(200, "abc123extra", "abc123").is_err());
    }

    #[test]
    fn file_url_sits_at_domain_root() {
        assert_eq!(
            file_url("https://blog.example.com", "abc123"),
            "https://blog.example.com/abc123.txt"
        );
        assert_eq!(
            file_url("http://blog.example.com/", "abc123"),
            "https://blog.example.com/abc123.txt"
        );
    }
}
