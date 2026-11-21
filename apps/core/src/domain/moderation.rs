//! 审核留痕：谁在什么时候对哪条评论做了什么。

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Event {
    pub id: Uuid,
    pub comment_id: Option<Uuid>,
    pub actor_kind: String,
    pub actor_id: Option<String>,
    pub action: String,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub thread_path: Option<String>,
    pub excerpt: Option<String>,
}

/// 站点最近的审核动作，最新在前。
pub async fn recent(state: &AppState, site_id: Uuid, limit: i64) -> anyhow::Result<Vec<Event>> {
    let rows: Vec<Event> = sqlx::query_as(
        "select e.id, e.comment_id, e.actor_kind, e.actor_id, e.action, e.reason, \
                e.created_at, t.path as thread_path, left(c.body_md, 80) as excerpt \
           from moderation_event e \
           left join comment c on c.id = e.comment_id \
           left join thread t on t.id = c.thread_id \
          where e.site_id = $1 \
          order by e.created_at desc \
          limit $2",
    )
    .bind(site_id)
    .bind(limit)
    .fetch_all(state.db())
    .await?;
    Ok(rows)
}

/// 某条评论的完整历史，按时间正序。
pub async fn for_comment(
    state: &AppState,
    site_id: Uuid,
    comment_id: Uuid,
) -> anyhow::Result<Vec<Event>> {
    let rows: Vec<Event> = sqlx::query_as(
        "select e.id, e.comment_id, e.actor_kind, e.actor_id, e.action, e.reason, \
                e.created_at, null::text as thread_path, null::text as excerpt \
           from moderation_event e \
          where e.site_id = $1 and e.comment_id = $2 \
          order by e.created_at asc",
    )
    .bind(site_id)
    .bind(comment_id)
    .fetch_all(state.db())
    .await?;
    Ok(rows)
}

/// 中文标签，页面直接显示别暴露库里的英文。
pub fn label(action: &str) -> &'static str {
    match action {
        "visible" => "放行",
        "hidden" => "隐藏",
        "spam" => "垃圾",
        "deleted" => "删除",
        "restored" => "恢复",
        _ => "改动",
    }
}

/// 库里记的是 `user`/`agent`/`system`，页面显示成中文。
pub fn actor(kind: &str) -> &'static str {
    match kind {
        "user" | "owner" => "站点主人",
        "agent" => "agent",
        "system" => "系统",
        _ => "自动",
    }
}
