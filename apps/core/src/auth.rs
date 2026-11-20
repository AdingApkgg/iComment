//! 认证：agent bearer token 与 device authorization 流。
//!
//! token 以 SHA-256 摘要入库，明文只在签发那一刻出现；`tsa_` 前缀便于泄漏扫描。

use axum::{extract::FromRequestParts, http::request::Parts};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

pub const TOKEN_PREFIX: &str = "tsa_";
const DEVICE_TTL_SECS: i64 = 600;
const USER_CODE_ALPHABET: &[u8] = b"BCDFGHJKLMNPQRSTVWXZ";

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn random_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    format!("{TOKEN_PREFIX}{}", bs58(&buf))
}

/// base58 编码（无依赖，避免额外 crate）。
fn bs58(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut digits: Vec<u8> = Vec::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        let mut carry = u32::from(byte);
        for digit in &mut digits {
            let value = u32::from(*digit) * 256 + carry;
            *digit = (value % 58) as u8;
            carry = value / 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    for &byte in bytes {
        if byte == 0 {
            digits.push(0);
        } else {
            break;
        }
    }
    digits
        .iter()
        .rev()
        .map(|d| ALPHABET[*d as usize] as char)
        .collect()
}

fn random_user_code() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let pick = |rng: &mut rand::rngs::ThreadRng| {
        let idx = rng.random_range(0..USER_CODE_ALPHABET.len());
        USER_CODE_ALPHABET[idx] as char
    };
    let a: String = (0..4)
        .map(|_| pick(&mut rng))
        .collect::<String>()
        .to_owned();
    let b: String = (0..4).map(|_| pick(&mut rng)).collect();
    format!("{a}-{b}")
}

/// 调用者身份，由中间件注入。
#[derive(Debug, Clone)]
pub struct Agent {
    pub account_id: Uuid,
    pub token_id: Uuid,
    /// `None` = 账号级 token，可建站；`Some` = 站点级 token。
    pub site_id: Option<Uuid>,
}

impl Agent {
    /// 站点 scope 校验：站点级 token 只能动自己的站。
    pub fn allows_site(&self, site_id: Uuid) -> bool {
        self.site_id.is_none_or(|bound| bound == site_id)
    }
}

impl FromRequestParts<AppState> for Agent {
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|v| v.starts_with(TOKEN_PREFIX))
            .ok_or_else(|| Problem::code(Code::Unauthenticated, "missing or malformed bearer"))?;

        let hash = hash_token(header);
        let row: Option<(Uuid, Uuid, Option<Uuid>)> = sqlx::query_as(
            "select id, account_id, site_id from agent_token \
             where token_hash = $1 and revoked_at is null",
        )
        .bind(&hash)
        .fetch_optional(state.db())
        .await?;

        let (token_id, account_id, site_id) =
            row.ok_or_else(|| Problem::code(Code::Unauthenticated, "token unknown or revoked"))?;

        sqlx::query("update agent_token set last_used_at = now() where id = $1")
            .bind(token_id)
            .execute(state.db())
            .await?;

        Ok(Self {
            account_id,
            token_id,
            site_id,
        })
    }
}

pub struct DeviceFlow {
    pub device_code: String,
    pub user_code: String,
    pub expires_in: i64,
}

/// 开一个 device authorization。账号在用户于验证页确认后才绑定。
pub async fn start_device_flow(db: &PgPool) -> Result<DeviceFlow> {
    let device_code = random_token();
    let user_code = random_user_code();
    let expires_at = Utc::now() + Duration::seconds(DEVICE_TTL_SECS);

    sqlx::query(
        "insert into device_code (device_code, user_code, status, expires_at) \
         values ($1, $2, 'pending', $3)",
    )
    .bind(&device_code)
    .bind(&user_code)
    .bind(expires_at)
    .execute(db)
    .await?;

    Ok(DeviceFlow {
        device_code,
        user_code,
        expires_in: DEVICE_TTL_SECS,
    })
}

pub enum DeviceOutcome {
    Pending,
    Approved { token: String, account_id: Uuid },
    Expired,
}

/// 轮询换 token。批准后签发新 token 并消费掉 device_code。
pub async fn poll_device_token(db: &PgPool, device_code: &str) -> Result<DeviceOutcome> {
    let row: Option<(String, Option<Uuid>, chrono::DateTime<Utc>)> = sqlx::query_as(
        "select status, account_id, expires_at from device_code where device_code = $1",
    )
    .bind(device_code)
    .fetch_optional(db)
    .await?;

    let Some((status, account_id, expires_at)) = row else {
        return Err(Problem::code(Code::Unauthenticated, "unknown device_code"));
    };
    if expires_at < Utc::now() {
        return Ok(DeviceOutcome::Expired);
    }
    match (status.as_str(), account_id) {
        ("pending", _) => Ok(DeviceOutcome::Pending),
        ("denied", _) => Err(Problem::code(Code::Forbidden, "user denied the request")),
        ("approved", Some(account_id)) => {
            let token = random_token();
            let mut tx = db.begin().await?;
            sqlx::query(
                "insert into agent_token (id, account_id, token_hash, label) values ($1, $2, $3, $4)",
            )
            .bind(Uuid::now_v7())
            .bind(account_id)
            .bind(hash_token(&token))
            .bind("device flow")
            .execute(&mut *tx)
            .await?;
            // 一次性：消费后重放拿不到第二个 token
            sqlx::query("delete from device_code where device_code = $1")
                .bind(device_code)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(DeviceOutcome::Approved { token, account_id })
        }
        ("approved", None) => Err(Problem::code(
            Code::Internal,
            "approved device_code without account",
        )),
        (other, _) => Err(Problem::code(
            Code::Internal,
            format!("unexpected device status {other}"),
        )),
    }
}

/// 人（网页）那一侧：用 user_code 批准。
pub async fn approve_device_code(db: &PgPool, user_code: &str, account_id: Uuid) -> Result<()> {
    let normalized = user_code.trim().to_uppercase().replace(' ', "");
    let affected = sqlx::query(
        "update device_code set status = 'approved', account_id = $2 \
         where upper(user_code) = $1 and status = 'pending' and expires_at > now()",
    )
    .bind(&normalized)
    .bind(account_id)
    .execute(db)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(Problem::code(
            Code::Validation,
            "user_code unknown, already used or expired",
        ));
    }
    Ok(())
}

/// 签发站点级 token（建站后给 agent 用）。
pub async fn issue_site_token(
    db: &PgPool,
    account_id: Uuid,
    site_id: Uuid,
    label: &str,
) -> Result<String> {
    let token = random_token();
    sqlx::query(
        "insert into agent_token (id, account_id, site_id, token_hash, label) \
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(account_id)
    .bind(site_id)
    .bind(hash_token(&token))
    .bind(label)
    .execute(db)
    .await?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_prefixed_and_unique() {
        let a = random_token();
        let b = random_token();
        assert!(a.starts_with(TOKEN_PREFIX));
        assert_ne!(a, b);
        assert_eq!(hash_token("x").len(), 64);
    }

    #[test]
    fn user_codes_are_grouped() {
        let code = random_user_code();
        assert_eq!(code.len(), 9);
        assert_eq!(code.chars().nth(4), Some('-'));
        assert!(code.chars().filter(|c| c.is_ascii_alphabetic()).count() == 8);
    }
}
