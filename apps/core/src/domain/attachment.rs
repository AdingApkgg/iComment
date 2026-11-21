//! 附图：上传登记（`pending`）→ 发评论时绑定。没绑上的过期记录由清理任务删。
//!
//! 存的是内容哈希（`blob.hash`，见 `blob`），所以同一张图被多条评论用也只存一份；
//! 引用关系就是这个表本身。

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    blob,
    error::{Code, Problem, Result},
    state::AppState,
};

pub const MAX_AGE_SECS: i64 = 86_400;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct Attachment {
    pub id: Uuid,
    pub position: i32,
    pub hash: String,
    pub bytes: i32,
    pub width: i32,
    pub height: i32,
}

/// 登记一次上传；`comment_id` 为空表示还没绑定。
pub async fn record(
    state: &AppState,
    site_id: Uuid,
    id: Uuid,
    hash: &str,
    bytes: i32,
    width: i32,
    height: i32,
) -> Result<()> {
    sqlx::query(
        "insert into attachment (id, site_id, position, hash, bytes, width, height) \
         values ($1, $2, -1, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(site_id)
    .bind(hash)
    .bind(bytes)
    .bind(width)
    .bind(height)
    .execute(state.db())
    .await?;
    Ok(())
}

/// 把附件绑到评论上，按 `body_md` 里的出现顺序定位置。
pub async fn bind(state: &AppState, site_id: Uuid, comment_id: Uuid, ids: &[Uuid]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut tx = state.db().begin().await?;
    for (position, id) in ids.iter().enumerate() {
        // 只认这个站点、还没被别人绑走的
        sqlx::query(
            "update attachment set comment_id = $1, position = $2 \
             where id = $3 and site_id = $4 and comment_id is null",
        )
        .bind(comment_id)
        .bind(position as i32)
        .bind(id)
        .bind(site_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 一次查一页评论的附图，避免 N+1。
pub async fn batch(
    state: &AppState,
    site_id: Uuid,
    comment_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<Attachment>>> {
    let mut out: std::collections::HashMap<Uuid, Vec<Attachment>> =
        std::collections::HashMap::new();
    if comment_ids.is_empty() {
        return Ok(out);
    }
    // 先从整行取完，再按评论分组（`query_as` 不会自动给元组拆 `FromRow`）
    let rows: Vec<(Uuid, Uuid, i32, String, i32, i32, i32)> = sqlx::query_as(
        "select comment_id, id, position, hash, bytes, width, height from attachment \
         where site_id = $1 and comment_id = any($2) order by position",
    )
    .bind(site_id)
    .bind(comment_ids)
    .fetch_all(state.db())
    .await?;
    for (comment_id, id, position, hash, bytes, width, height) in rows {
        out.entry(comment_id).or_default().push(Attachment {
            id,
            position,
            hash,
            bytes,
            width,
            height,
        });
    }
    Ok(out)
}

/// 单条评论的附图，按出现顺序。
pub async fn for_comment(state: &AppState, comment_id: Uuid) -> Result<Vec<Attachment>> {
    let rows = sqlx::query_as::<_, Attachment>(
        "select id, position, hash, bytes, width, height from attachment \
         where comment_id = $1 order by position",
    )
    .bind(comment_id)
    .fetch_all(state.db())
    .await?;
    Ok(rows)
}

/// 按 id 取附件（正文占位符展开时用）。
pub async fn by_ids(state: &AppState, ids: &[Uuid]) -> Result<Vec<Attachment>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, Attachment>(
        "select id, position, hash, bytes, width, height from attachment where id = any($1)",
    )
    .bind(ids)
    .fetch_all(state.db())
    .await?;
    Ok(rows)
}

/// 引用的附件必须都存在、都属于这个站点、且还没绑给别人。
pub async fn pending_owned(state: &AppState, site_id: Uuid, ids: &[Uuid]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let found: Vec<Uuid> = sqlx::query_scalar(
        "select id from attachment \
         where site_id = $1 and comment_id is null and id = any($2)",
    )
    .bind(site_id)
    .bind(ids)
    .fetch_all(state.db())
    .await?;
    if let Some(missing) = ids.iter().find(|id| !found.contains(id)) {
        return Err(Problem::code(
            Code::Validation,
            format!("attachment {missing} is not a pending upload of this site"),
        ));
    }
    Ok(())
}

/// 删掉超过一天还没绑定的登记，再清掉因此没人引用的 blob。
pub async fn sweep(state: &AppState) -> Result<u64> {
    let removed = sqlx::query_scalar::<_, Uuid>(
        "delete from attachment where comment_id is null \
         and created_at < now() - ($1 || ' seconds')::interval returning id",
    )
    .bind(MAX_AGE_SECS.to_string())
    .fetch_all(state.db())
    .await?
    .len();
    blob::sweep_orphans(state).await?;
    Ok(removed as u64)
}

/// 占位符 id → 真实地址。
pub fn link_map(rows: &[Attachment], base: &str) -> std::collections::HashMap<Uuid, String> {
    let base = base.trim_end_matches('/');
    rows.iter()
        .map(|a| (a.id, format!("{base}/media/{}", a.hash)))
        .collect()
}

/// 单个附件的绝对地址。
pub fn link(base: &str, hash: &str) -> String {
    format!("{}/media/{hash}", base.trim_end_matches('/'))
}

pub fn created_before(cutoff: DateTime<Utc>) -> DateTime<Utc> {
    cutoff - chrono::Duration::seconds(MAX_AGE_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_map_points_at_media() {
        let a = Attachment {
            id: Uuid::nil(),
            position: 0,
            hash: "abc123".into(),
            bytes: 1,
            width: 1,
            height: 1,
        };
        assert_eq!(
            link_map(&[a], "https://c.example/")
                .get(&Uuid::nil())
                .unwrap(),
            "https://c.example/media/abc123"
        );
    }
}
