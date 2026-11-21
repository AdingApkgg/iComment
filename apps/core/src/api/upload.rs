//! 上传与取图。上传成功后回一段 Markdown，直接插进 `body_md` 即可。

use axum::Json;
use axum::body::Body;
use axum::extract::multipart::Multipart;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    antispam, blob,
    domain::{attachment, site},
    error::{Code, Problem, Result},
    extract::{ClientIp, SiteOrigin},
    state::AppState,
    upload,
};

#[derive(Debug, Serialize)]
pub struct Stored {
    pub id: Uuid,
    pub url: String,
    /// 直接贴进 `body_md` 的片段
    pub markdown: String,
    pub width: i32,
    pub height: i32,
}

/// 收一张图并登记成待绑定附件。
#[axum::debug_handler]
pub async fn create(
    State(state): State<AppState>,
    SiteOrigin(origin): SiteOrigin,
    ClientIp(ip): ClientIp,
    mut multipart: Multipart,
) -> Result<Json<Stored>> {
    if !state.cfg().uploads_enabled() {
        return Err(Problem::code(Code::Validation, "uploads are disabled"));
    }
    let site = site::resolve(&state, &origin).await?;
    antispam::hit(
        &state,
        &format!("ic:rl:u:{}", ip.map(|v| v.to_string()).unwrap_or_default()),
        antispam::UPLOAD_HOURLY,
        3600,
    )
    .await?;

    let mut field = multipart
        .next_field()
        .await
        .map_err(|e| Problem::code(Code::Validation, format!("bad multipart: {e}")))?
        .ok_or_else(|| Problem::code(Code::Validation, "no file part"))?;

    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| Problem::code(Code::Validation, format!("unreadable upload: {e}")))?
    {
        buf.extend_from_slice(&chunk);
        if buf.len() > upload::MAX_FILE_BYTES {
            return Err(Problem::code(Code::Validation, "over 4 MiB"));
        }
    }
    if buf.is_empty() {
        return Err(Problem::code(Code::Validation, "empty upload"));
    }

    let kind = upload::sniff(&buf)
        .ok_or_else(|| Problem::code(Code::Validation, "only PNG, JPEG, WebP or GIF"))?;
    let (width, height) = upload::dimensions(kind, &buf).unwrap_or((0, 0));
    let hash = blob::put(&state, kind.ext(), &buf).await?;
    let id = Uuid::now_v7();
    // 占位符按 id 认（同一张图可以贴两次，各自独立绑定）
    attachment::record(&state, site.id, id, &hash, buf.len() as i32, width, height).await?;

    let url = attachment::link(&state.cfg().public_base_url, &hash);
    Ok(Json(Stored {
        id,
        markdown: format!("![](ic-attachment:{id})"),
        url,
        width,
        height,
    }))
}

/// 取图。`/media/{file}` 与站点无关，路径里只有随机 id。
#[axum::debug_handler]
pub async fn media(
    State(state): State<AppState>,
    Path(file): Path<String>,
    headers: HeaderMap,
) -> Result<Response> {
    let hash = file.split('.').next().unwrap_or(&file);
    let (bytes, content_type) = blob::get(&state, hash)
        .await
        .ok_or_else(|| Problem::code(Code::NotFound, "no such media"))?;

    // 内容不可变，etag 直接用哈希
    let etag = format!("\"{hash}\"");
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        return Ok(Response::builder()
            .status(axum::http::StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .expect("empty body is infallible"));
    }

    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::ETAG, etag)
        .body(Body::from(bytes))
        .map_err(|e| Problem::code(Code::Internal, e.to_string()))
}
