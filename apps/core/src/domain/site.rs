//! 站点：建站、按 origin 解析、改设置、删站。
//!
//! origin 是站点身份，唯一；删除是软删（保留 thread 与评论以便恢复）。

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    error::{Code, Problem, Result},
    normalize::normalize_origin,
    state::AppState,
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Site {
    pub id: Uuid,
    pub origin: String,
    pub name: Option<String>,
    pub require_approval: bool,
    pub challenge_mode: String,
    pub widget_version: String,
    pub site_key: String,
    pub created_at: DateTime<Utc>,
}

const COLS: &str =
    "id, origin, name, require_approval, challenge_mode, widget_version, site_key, created_at";

fn random_key() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 12];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub struct CreateSite {
    pub origin: String,
    pub name: Option<String>,
}

/// 建站。origin 归一化后冲突返回 400。
pub async fn create(state: &AppState, owner: Uuid, input: CreateSite) -> Result<Site> {
    let origin = normalize_origin(&input.origin)
        .ok_or_else(|| Problem::field("origin", "not a usable origin"))?;
    let name = input
        .name
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty());

    let site: Option<Site> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "insert into site (id, owner_id, origin, name, site_key, site_secret) \
         values ($1, $2, $3, $4, $5, $6) \
         on conflict (lower(origin)) where deleted_at is null do nothing \
         returning {COLS}"
    )))
    .bind(Uuid::now_v7())
    .bind(owner)
    .bind(&origin)
    .bind(&name)
    .bind(random_key())
    .bind(random_key())
    .fetch_optional(state.db())
    .await?;

    site.ok_or_else(|| Problem::field("origin", "already registered"))
}

pub async fn by_origin(state: &AppState, origin: &str) -> Result<Site> {
    let origin =
        normalize_origin(origin).ok_or_else(|| Problem::field("origin", "not a usable origin"))?;
    let site: Option<Site> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from site where lower(origin) = $1 and deleted_at is null"
    )))
    .bind(&origin)
    .fetch_optional(state.db())
    .await?;
    site.ok_or_else(|| Problem::code(Code::SiteNotFound, "site not registered"))
}

/// 按 widget 传来的定位串解析站点：`key:<site_key>` 或 origin。
pub async fn resolve(state: &AppState, locator: &str) -> Result<Site> {
    if let Some(key) = locator.strip_prefix(crate::extract::KEY_PREFIX) {
        return by_key(state, key).await;
    }
    by_origin(state, locator).await
}

pub async fn by_key(state: &AppState, key: &str) -> Result<Site> {
    let site: Option<Site> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from site where site_key = $1 and deleted_at is null"
    )))
    .bind(key)
    .fetch_optional(state.db())
    .await?;
    site.ok_or_else(|| Problem::code(Code::SiteNotFound, "site_key not registered"))
}

pub async fn by_id(state: &AppState, id: Uuid) -> Result<Site> {
    let site: Option<Site> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from site where id = $1 and deleted_at is null"
    )))
    .bind(id)
    .fetch_optional(state.db())
    .await?;
    site.ok_or_else(|| Problem::code(Code::SiteNotFound, "site not found"))
}

/// 配置视图：给 agent 看，不含 secret。
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct SiteView {
    pub id: Uuid,
    pub origin: String,
    pub name: Option<String>,
    pub require_approval: bool,
    pub challenge_mode: String,
    pub widget_version: String,
    pub site_key: String,
    pub created_at: DateTime<Utc>,
}

pub async fn view(state: &AppState, id: Uuid) -> Result<SiteView> {
    let view: Option<SiteView> = sqlx::query_as(
        "select id, origin, name, require_approval, challenge_mode, widget_version, site_key, \
         created_at from site where id = $1 and deleted_at is null",
    )
    .bind(id)
    .fetch_optional(state.db())
    .await?;
    view.ok_or_else(|| Problem::code(Code::SiteNotFound, "site not found"))
}

#[derive(Debug, Default)]
pub struct SiteUpdate {
    pub name: Option<String>,
    pub require_approval: Option<bool>,
    pub challenge_mode: Option<String>,
}

pub async fn update(state: &AppState, id: Uuid, patch: SiteUpdate) -> Result<SiteView> {
    if patch
        .challenge_mode
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "always" | "off" | "suspicious"))
    {
        return Err(Problem::field(
            "challenge_mode",
            "must be always, off or suspicious",
        ));
    }
    if patch.challenge_mode.as_deref().is_some_and(|m| m != "off")
        && state.cfg().turnstile_secret.is_none()
    {
        return Err(Problem::field(
            "challenge_mode",
            "server has no Turnstile secret configured",
        ));
    }

    sqlx::query(
        "update site set \
         name = coalesce($2, name), \
         require_approval = coalesce($3, require_approval), \
         challenge_mode = coalesce($4, challenge_mode) \
         where id = $1 and deleted_at is null",
    )
    .bind(id)
    .bind(
        patch
            .name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty()),
    )
    .bind(patch.require_approval)
    .bind(patch.challenge_mode)
    .execute(state.db())
    .await?;

    view(state, id).await
}

pub async fn soft_delete(state: &AppState, id: Uuid) -> Result<()> {
    let affected =
        sqlx::query("update site set deleted_at = now() where id = $1 and deleted_at is null")
            .bind(id)
            .execute(state.db())
            .await?
            .rows_affected();
    if affected == 0 {
        return Err(Problem::code(Code::SiteNotFound, "site not found"));
    }
    Ok(())
}

/// 站点列表（账号级 token 用）。
pub async fn list_for_owner(state: &AppState, owner: Uuid) -> Result<Vec<SiteView>> {
    let rows: Vec<SiteView> = sqlx::query_as(
        "select id, origin, name, require_approval, challenge_mode, widget_version, site_key, \
         created_at from site where owner_id = $1 and deleted_at is null order by created_at",
    )
    .bind(owner)
    .fetch_all(state.db())
    .await?;
    Ok(rows)
}
