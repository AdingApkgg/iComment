//! 附图：上传登记（`pending`）→ 发评论时绑定。没绑上的过期记录由清理任务删。

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

pub const MAX_AGE_SECS: i64 = 86_400;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct Attachment {
    pub id: Uuid,
    pub position: i32,
    pub path: String,
    pub bytes: i32,
    pub width: i32,
    pub height: i32,
}

/// 登记一次上传；`comment_id` 为空表示还没绑定。
pub async fn record(
    state: &AppState,
    site_id: Uuid,
    id: Uuid,
    path: &str,
    bytes: i32,
    width: i32,
    height: i32,
) -> Result<()> {
    sqlx::query(
        "insert into attachment (id, site_id, position, path, bytes, width, height) \
         values ($1, $2, -1, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(site_id)
    .bind(path)
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
    let rows: Vec<(Uuid, Attachment)> = sqlx::query_as(
        "select comment_id, id, position, path, bytes, width, height from attachment \
         where site_id = $1 and comment_id = any($2) order by comment_id, position",
    )
    .bind(site_id)
    .bind(comment_ids)
    .fetch_all(state.db())
    .await?
    .into_iter()
    .map(|row: (Uuid, Uuid, i32, String, i32, i32, i32)| {
        let (comment_id, id, position, path, bytes, width, height) = row;
        (
            comment_id,
            Attachment {
                id,
                position,
                path,
                bytes,
                width,
                height,
            },
        )
    })
    .collect();
    for (comment_id, attachment) in rows {
        out.entry(comment_id).or_default().push(attachment);
    }
    for list in out.values_mut() {
        list.sort_by_key(|a| a.position);
    }
    Ok(out)
}

/// 单条评论的附图（按位置）。
pub async fn for_comment(state: &AppState, comment_id: Uuid) -> Result<Vec<Attachment>> {
    Ok(sqlx::query_as(
        "select id, position, path, bytes, width, height from attachment \
         where comment_id = $1 order by position",
    )
    .bind(comment_id)
    .fetch_all(state.db())
    .await?)
}

/// 删除某一站一小时内还没绑定的上传（保留期外）。
pub async fn sweep(state: &AppState) -> Result<u64> {
    let rows: Vec<String> = sqlx::query_scalar(
        "delete from attachment where comment_id is null \
         and created_at < now() - ($1 || ' seconds')::interval returning path",
    )
    .bind(MAX_AGE_SECS.to_string())
    .fetch_all(state.db())
    .await?;
    for path in &rows {
        let _ =
            tokio::fs::remove_file(std::path::Path::new(&state.cfg().upload_dir).join(path)).await;
    }
    Ok(rows.len() as u64)
}

/// 校验：评论里引用的附件必须属于本站在等待绑定的记录。
pub async fn pending_owned(state: &AppState, site_id: Uuid, ids: &[Uuid]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let found: Vec<Uuid> = sqlx::query_scalar(
        "select id from attachment where site_id = $1 and comment_id is null and id = any($2)",
    )
    .bind(site_id)
    .bind(ids)
    .fetch_all(state.db())
    .await?;
    match ids.iter().find(|id| !found.contains(id)) {
        Some(missing) => Err(Problem::code(
            Code::Validation,
            format!("attachment {missing} is not a pending upload of this site"),
        )),
        None => Ok(()),
    }
}

/// 按 id 取记录（顺序不保证）。
pub async fn by_ids(state: &AppState, ids: &[Uuid]) -> Result<Vec<Attachment>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<Attachment> = sqlx::query_as(
        "select id, position, path, bytes, width, height from attachment where id = any($1)",
    )
    .bind(ids)
    .fetch_all(state.db())
    .await?;
    Ok(rows)
}

/// 给渲染用的 id → 绝对地址。嵌入在别人的站上，相对路径会指向博客域名，必须带站点基址。
pub fn link_map(rows: &[Attachment], base: &str) -> std::collections::HashMap<Uuid, String> {
    let base = base.trim_end_matches('/');
    rows.iter()
        .map(|a| (a.id, format!("{base}/media/{}", a.path)))
        .collect()
}

/// 单个附件的绝对地址。
pub fn link(base: &str, path: &str) -> String {
    format!("{}/media/{path}", base.trim_end_matches('/'))
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
            path: "x.png".into(),
            bytes: 1,
            width: 1,
            height: 1,
        };
        assert_eq!(
            link_map(&[a], "https://c.example/")
                .get(&Uuid::nil())
                .unwrap(),
            "https://c.example/media/x.png"
        );
    }
}
