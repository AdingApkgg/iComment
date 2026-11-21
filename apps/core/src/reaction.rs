//! 表情反应。只对可见/待审的评论开放，我是谁由 cookie 或客户端 id 决定。

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

/// 固定五种，前端按键名渲染。
pub const KINDS: [&str; 5] = ["up", "heart", "haha", "thinking", "party"];

pub fn is_kind(kind: &str) -> bool {
    KINDS.contains(&kind)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Tally {
    /// 每种表情的票数，没人投的键不出现
    pub counts: BTreeMap<&'static str, i64>,
    /// 当前投票人投过的
    pub mine: Vec<&'static str>,
}

/// 一条评论的全部票数 + 自己的票。
pub async fn tally(state: &AppState, comment_id: Uuid, voter: Option<&str>) -> Result<Tally> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "select kind::text, count(*) from reaction where comment_id = $1 group by kind",
    )
    .bind(comment_id)
    .fetch_all(state.db())
    .await?;

    let mut counts = BTreeMap::new();
    for (kind, n) in rows {
        if let Some(known) = KINDS.iter().find(|k| **k == kind) {
            counts.insert(*known, n);
        }
    }

    let mine: Vec<String> = match voter {
        Some(voter) => {
            sqlx::query_scalar(
                "select kind::text from reaction where comment_id = $1 and voter = $2",
            )
            .bind(comment_id)
            .bind(voter)
            .fetch_all(state.db())
            .await?
        }
        None => Vec::new(),
    };

    Ok(Tally {
        counts,
        mine: KINDS
            .iter()
            .copied()
            .filter(|k| mine.iter().any(|m| m == k))
            .collect(),
    })
}

/// 一次查多条评论的票数（列表页用），返回 comment_id → 票数。
pub async fn batch(
    state: &AppState,
    comment_ids: &[Uuid],
    voter: Option<&str>,
) -> Result<BTreeMap<Uuid, Tally>> {
    let mut map: BTreeMap<Uuid, Tally> = BTreeMap::new();
    if comment_ids.is_empty() {
        return Ok(map);
    }

    let rows: Vec<(Uuid, String, i64)> = sqlx::query_as(
        "select comment_id, kind::text, count(*) from reaction \
          where comment_id = any($1) group by comment_id, kind",
    )
    .bind(comment_ids)
    .fetch_all(state.db())
    .await?;

    for (comment_id, kind, n) in rows {
        if let Some(known) = KINDS.iter().find(|k| **k == kind) {
            map.entry(comment_id).or_default().counts.insert(known, n);
        }
    }

    if let Some(voter) = voter {
        let mine: Vec<(Uuid, String)> = sqlx::query_as(
            "select comment_id, kind::text from reaction where comment_id = any($1) and voter = $2",
        )
        .bind(comment_ids)
        .bind(voter)
        .fetch_all(state.db())
        .await?;
        for (comment_id, kind) in mine {
            if let Some(known) = KINDS.iter().find(|k| **k == kind) {
                map.entry(comment_id).or_default().mine.push(known);
            }
        }
    }

    Ok(map)
}

/// 投票，返回投票后的票数。重复投同一种不变（on conflict do nothing）。
pub async fn add(
    state: &AppState,
    site_id: Uuid,
    comment_id: Uuid,
    kind: &str,
    voter: &str,
) -> Result<Tally> {
    if !is_kind(kind) {
        return Err(Problem::code(Code::Validation, "unknown reaction"));
    }
    ensure_listed(state, site_id, comment_id).await?;

    sqlx::query(
        "insert into reaction (comment_id, site_id, kind, voter) values ($1, $2, $3::reaction_kind, $4) \
         on conflict do nothing",
    )
    .bind(comment_id)
    .bind(site_id)
    .bind(kind)
    .bind(voter)
    .execute(state.db())
    .await?;

    tally(state, comment_id, Some(voter)).await
}

/// 撤票，没投过也当成功（幂等）。
pub async fn remove(
    state: &AppState,
    site_id: Uuid,
    comment_id: Uuid,
    kind: &str,
    voter: &str,
) -> Result<Tally> {
    if !is_kind(kind) {
        return Err(Problem::code(Code::Validation, "unknown reaction"));
    }
    ensure_listed(state, site_id, comment_id).await?;

    sqlx::query(
        "delete from reaction where comment_id = $1 and kind = $2::reaction_kind and voter = $3",
    )
    .bind(comment_id)
    .bind(kind)
    .bind(voter)
    .execute(state.db())
    .await?;

    tally(state, comment_id, Some(voter)).await
}

/// 只有还挂在站上的、没被删的评论能投。
async fn ensure_listed(state: &AppState, site_id: Uuid, comment_id: Uuid) -> Result<()> {
    let status: Option<String> =
        sqlx::query_scalar("select status from comment where id = $1 and site_id = $2")
            .bind(comment_id)
            .bind(site_id)
            .fetch_optional(state.db())
            .await?;
    match status.as_deref() {
        Some("visible" | "pending") => Ok(()),
        Some("deleted") => Err(Problem::code(Code::CommentNotFound, "comment deleted")),
        Some(_) => Err(Problem::code(Code::Validation, "comment is not listed")),
        None => Err(Problem::code(Code::CommentNotFound, "comment not found")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_kinds_pass() {
        assert!(is_kind("up"));
        assert!(is_kind("party"));
        assert!(!is_kind("clap"));
        assert!(!is_kind(""));
    }
}
