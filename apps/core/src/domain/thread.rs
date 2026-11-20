//! thread：一个 (site, path) 一条，惰性创建。

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    error::{Code, Problem, Result},
    normalize::normalize_path,
    state::AppState,
};

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct Thread {
    pub id: Uuid,
    pub site_id: Uuid,
    pub path: String,
    pub title: Option<String>,
    pub comment_count: i32,
    pub last_at: Option<DateTime<Utc>>,
}

/// 取或建 thread。并发首评靠 unique (site_id, path) 兜底。
pub async fn get_or_create(
    state: &AppState,
    site_id: Uuid,
    raw_path: &str,
    title: Option<&str>,
) -> Result<Thread> {
    let path = normalize_path(raw_path)
        .ok_or_else(|| Problem::field("path", "path too long or unusable"))?;

    let existing: Option<Thread> =
        sqlx::query_as("select * from thread where site_id = $1 and path = $2")
            .bind(site_id)
            .bind(&path)
            .fetch_optional(state.db())
            .await?;
    if let Some(thread) = existing {
        return Ok(thread);
    }

    let inserted: Option<Thread> = sqlx::query_as(
        "insert into thread (id, site_id, path, title) values ($1, $2, $3, $4) \
         on conflict (site_id, path) do nothing returning *",
    )
    .bind(Uuid::now_v7())
    .bind(site_id)
    .bind(&path)
    .bind(title.map(str::trim).filter(|t| !t.is_empty()))
    .fetch_optional(state.db())
    .await?;

    match inserted {
        Some(thread) => Ok(thread),
        None => sqlx::query_as("select * from thread where site_id = $1 and path = $2")
            .bind(site_id)
            .bind(&path)
            .fetch_one(state.db())
            .await
            .map_err(Into::into),
    }
}

pub async fn by_id(state: &AppState, site_id: Uuid, id: Uuid) -> Result<Thread> {
    let thread: Option<Thread> =
        sqlx::query_as("select * from thread where id = $1 and site_id = $2")
            .bind(id)
            .bind(site_id)
            .fetch_optional(state.db())
            .await?;
    thread.ok_or_else(|| Problem::code(Code::ThreadNotFound, "thread not found"))
}

/// 计数与最后时间在写评论时增量维护，避免列表页 count(*)。
pub async fn bump(state: &AppState, thread_id: Uuid, delta: i32, at: DateTime<Utc>) -> Result<()> {
    sqlx::query(
        "update thread set comment_count = greatest(comment_count + $2, 0), last_at = $3 \
         where id = $1",
    )
    .bind(thread_id)
    .bind(delta)
    .bind(at)
    .execute(state.db())
    .await?;
    Ok(())
}

pub async fn rename(state: &AppState, id: Uuid, title: &str) -> Result<()> {
    sqlx::query("update thread set title = $2 where id = $1")
        .bind(id)
        .bind(title.trim())
        .execute(state.db())
        .await?;
    Ok(())
}
