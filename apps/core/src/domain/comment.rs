//! 评论：写入、分页读取、审核状态机。
//!
//! 状态迁移只允许经 [`transition`]，保证每次变化都留 moderation_event。

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

pub const MAX_BODY_BYTES: usize = 8 * 1024;
pub const MAX_DEPTH: i32 = 5;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct Comment {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub body_md: String,
    pub body_html: String,
    pub author_kind: String,
    pub author_uid: Option<String>,
    pub author_name: Option<String>,
    pub author_avatar: Option<String>,
    pub author_email: Option<String>,
    pub status: String,
    pub identity_unverified: bool,
    pub created_at: DateTime<Utc>,
}

const COLS: &str = "id, thread_id, parent_id, body_md, body_html, author_kind, author_uid, \
                    author_name, author_avatar, author_email, status, identity_unverified, created_at";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Visible,
    Hidden,
    Spam,
    Deleted,
}

impl Transition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::Hidden => "hidden",
            Self::Spam => "spam",
            Self::Deleted => "deleted",
        }
    }

    /// 目标状态是否从当前状态可达。
    fn allowed_from(self, current: &str) -> bool {
        !matches!((current, self), ("deleted", _) | ("visible", Self::Visible))
    }
}

pub struct NewComment {
    pub site_id: Uuid,
    pub thread_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub body_md: String,
    pub body_html: String,
    pub author_kind: &'static str,
    pub author_uid: Option<String>,
    pub author_name: Option<String>,
    pub author_avatar: Option<String>,
    pub author_email: Option<String>,
    pub ip_hash: Option<String>,
    pub ua_hash: Option<String>,
    pub idempotency_key: Option<String>,
    pub status: &'static str,
    pub identity_unverified: bool,
}

/// 插入评论。命中 idempotency_key 唯一索引时返回已有行。
pub async fn insert(state: &AppState, new: NewComment) -> Result<(Comment, bool)> {
    let id = Uuid::now_v7();
    let inserted: Option<Comment> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "insert into comment (id, site_id, thread_id, parent_id, body_md, body_html, author_kind, \
         author_uid, author_name, author_avatar, author_email, ip_hash, ua_hash, \
         idempotency_key, status, identity_unverified) \
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) \
         on conflict do nothing returning {COLS}"
    )))
    .bind(id)
    .bind(new.site_id)
    .bind(new.thread_id)
    .bind(new.parent_id)
    .bind(&new.body_md)
    .bind(&new.body_html)
    .bind(new.author_kind)
    .bind(&new.author_uid)
    .bind(&new.author_name)
    .bind(&new.author_avatar)
    .bind(&new.author_email)
    .bind(&new.ip_hash)
    .bind(&new.ua_hash)
    .bind(&new.idempotency_key)
    .bind(new.status)
    .bind(new.identity_unverified)
    .fetch_optional(state.db())
    .await?;

    if let Some(comment) = inserted {
        return Ok((comment, true));
    }

    let key = new
        .idempotency_key
        .ok_or_else(|| Problem::code(Code::Validation, "insert conflict without key"))?;
    let existing: Comment = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from comment where site_id = $1 and idempotency_key = $2"
    )))
    .bind(new.site_id)
    .bind(&key)
    .fetch_one(state.db())
    .await?;
    Ok((existing, false))
}

pub async fn by_id(state: &AppState, site_id: Uuid, id: Uuid) -> Result<Comment> {
    let comment: Option<Comment> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from comment where id = $1 and site_id = $2"
    )))
    .bind(id)
    .bind(site_id)
    .fetch_optional(state.db())
    .await?;
    comment.ok_or_else(|| Problem::code(Code::CommentNotFound, "comment not found"))
}

/// 父评论深度校验：与父同级或更深，超过 MAX_DEPTH 挂到上限层级。
pub async fn resolve_parent(
    state: &AppState,
    site_id: Uuid,
    thread_id: Uuid,
    parent_id: Option<Uuid>,
) -> Result<(Option<Uuid>, i32)> {
    let Some(parent_id) = parent_id else {
        return Ok((None, 0));
    };

    let mut chain: Vec<Uuid> = Vec::new();
    let mut cursor = Some(parent_id);
    while let Some(id) = cursor {
        let row: Option<(Uuid, Option<Uuid>, Uuid)> = sqlx::query_as(
            "select id, parent_id, thread_id from comment where id = $1 and site_id = $2",
        )
        .bind(id)
        .bind(site_id)
        .fetch_optional(state.db())
        .await?;
        let Some((found_id, next_parent, found_thread)) = row else {
            return Err(Problem::code(Code::CommentNotFound, "parent not found"));
        };
        if found_thread != thread_id {
            return Err(Problem::field(
                "parent_id",
                "parent lives in another thread",
            ));
        }
        chain.push(found_id);
        cursor = next_parent;
        if chain.len() > 32 {
            break;
        }
    }

    let depth = chain.len() as i32;
    if depth < MAX_DEPTH {
        return Ok((Some(parent_id), depth));
    }
    // 超深：挂到第 MAX_DEPTH 层的祖先
    let anchor = *chain.get((MAX_DEPTH - 1) as usize).unwrap_or(&parent_id);
    Ok((Some(anchor), MAX_DEPTH))
}

pub struct Page {
    pub comments: Vec<Comment>,
    pub next_cursor: Option<DateTime<Utc>>,
}

/// 平铺分页（最新在前），客户端自己按 parent_id 组树。
pub async fn page(
    state: &AppState,
    thread_id: Uuid,
    include_pending: bool,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Page> {
    let limit = limit.clamp(1, 100);
    let statuses: &[&str] = if include_pending {
        &["visible", "pending"]
    } else {
        &["visible"]
    };

    let rows: Vec<Comment> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from comment \
         where thread_id = $1 and status = any($2) and ($3::timestamptz is null or created_at < $3) \
         order by created_at desc limit $4"
    )))
    .bind(thread_id)
    .bind(statuses)
    .bind(before)
    .bind(limit + 1)
    .fetch_all(state.db())
    .await?;

    let next_cursor = if rows.len() as i64 > limit {
        rows.get(limit as usize - 1).map(|c| c.created_at)
    } else {
        None
    };
    let mut comments = rows;
    comments.truncate(limit as usize);
    Ok(Page {
        comments,
        next_cursor,
    })
}

/// 可见（含待审）评论总数。
pub async fn count(state: &AppState, thread_id: Uuid) -> Result<i64> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from comment where thread_id = $1 and status = any($2)",
    )
    .bind(thread_id)
    .bind(&["visible", "pending"][..])
    .fetch_one(state.db())
    .await?;
    Ok(n)
}

/// 直连子回复（深度 1 跳），给深链单条视图用。
pub async fn children(state: &AppState, thread_id: Uuid, parent_id: Uuid) -> Result<Vec<Comment>> {
    let rows: Vec<Comment> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select {COLS} from comment \
         where thread_id = $1 and parent_id = $2 and status = any($3) \
         order by created_at asc limit 100"
    )))
    .bind(thread_id)
    .bind(parent_id)
    .bind(&["visible", "pending"][..])
    .fetch_all(state.db())
    .await?;
    Ok(rows)
}

/// 状态迁移 + 事件。返回是否真的改了。
pub async fn transition(
    state: &AppState,
    site_id: Uuid,
    id: Uuid,
    to: Transition,
    actor_kind: &str,
    actor_id: Option<&str>,
    reason: Option<&str>,
) -> Result<Comment> {
    let current = by_id(state, site_id, id).await?;
    if !to.allowed_from(&current.status) {
        return Err(Problem::code(
            Code::Validation,
            format!("cannot move {} -> {}", current.status, to.as_str()),
        ));
    }

    let mut tx = state.db().begin().await?;
    let updated: Option<Comment> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "update comment set status = $3 where id = $1 and site_id = $2 and status = $4 \
         returning {COLS}"
    )))
    .bind(id)
    .bind(site_id)
    .bind(to.as_str())
    .bind(&current.status)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(updated) = updated else {
        return Err(Problem::code(
            Code::Validation,
            "status changed concurrently",
        ));
    };

    sqlx::query(
        "insert into moderation_event (id, site_id, comment_id, actor_kind, actor_id, action, reason) \
         values ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(Uuid::now_v7())
    .bind(site_id)
    .bind(id)
    .bind(actor_kind)
    .bind(actor_id)
    .bind(to.as_str())
    .bind(reason)
    .execute(&mut *tx)
    .await?;

    // 计数只跟「visible/pending 可见」走
    let was_listed = matches!(current.status.as_str(), "visible" | "pending");
    let is_listed = matches!(to, Transition::Visible) || updated.status == "pending";
    if was_listed != is_listed {
        let delta = if is_listed { 1 } else { -1 };
        sqlx::query(
            "update thread set comment_count = greatest(comment_count + $2, 0) where id = $1",
        )
        .bind(updated.thread_id)
        .bind(delta)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(updated)
}

/// 校验正文，返回 (trim 后 md, 字节数)。
pub fn validate_body(raw: &str) -> Result<String> {
    let body = raw.trim();
    if body.is_empty() {
        return Err(Problem::field("body_md", "内容不能为空"));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(Problem::field("body_md", "over 8 KiB"));
    }
    Ok(body.to_owned())
}
