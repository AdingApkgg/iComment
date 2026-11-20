//! 公开 API：widget 读取与写评论。鉴权靠 Origin，不需要 token。

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    antispam,
    domain::{comment, site, thread},
    error::{Code, Problem, Result},
    extract::{ClientIp, SiteOrigin},
    markdown,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct PublicConfig {
    pub site_id: Uuid,
    pub origin: String,
    pub name: Option<String>,
    pub require_approval: bool,
    pub challenge_mode: String,
    pub widget_version: String,
    pub turnstile_site_key: Option<String>,
    pub max_depth: i32,
    pub max_body_bytes: usize,
}

/// widget 冷启动：拿站点配置（含是否要 Turnstile）。
pub async fn config(
    State(state): State<AppState>,
    SiteOrigin(origin): SiteOrigin,
) -> Result<Json<PublicConfig>> {
    Ok(Json(site_config(&state, &origin).await?))
}

/// 建站后的嵌入页要预览同一份配置，所以单独抽出来。
pub async fn site_config(state: &AppState, origin: &str) -> Result<PublicConfig> {
    let site = site::resolve(state, origin).await?;
    Ok(PublicConfig {
        site_id: site.id,
        origin: site.origin,
        name: site.name,
        require_approval: site.require_approval,
        max_depth: comment::MAX_DEPTH,
        max_body_bytes: comment::MAX_BODY_BYTES,
        // 服务端没配 secret 时三态一律退化成 off，widget 也就不会去拉脚本
        challenge_mode: if state.cfg().turnstile_required() {
            site.challenge_mode
        } else {
            "off".to_owned()
        },
        turnstile_site_key: if state.cfg().turnstile_required() {
            state.cfg().turnstile_site_key.clone()
        } else {
            None
        },
        widget_version: site.widget_version,
    })
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub path: String,
    #[serde(default)]
    pub cursor: Option<DateTime<Utc>>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    30
}

#[derive(Debug, Serialize)]
pub struct CommentView {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub body_html: String,
    pub author_kind: String,
    pub author_name: Option<String>,
    pub author_avatar: Option<String>,
    pub status: String,
    pub identity_unverified: bool,
    pub created_at: DateTime<Utc>,
}

impl From<comment::Comment> for CommentView {
    fn from(c: comment::Comment) -> Self {
        Self {
            id: c.id,
            parent_id: c.parent_id,
            body_html: c.body_html,
            author_kind: c.author_kind,
            author_name: c.author_name,
            author_avatar: c.author_avatar,
            status: c.status,
            identity_unverified: c.identity_unverified,
            created_at: c.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CommentPage {
    pub total: i32,
    pub comments: Vec<CommentView>,
    pub next_cursor: Option<DateTime<Utc>>,
}

/// 读评论。`pending` 也会返回（自己刚发的能看见），widget 自行弱化显示。
pub async fn list(
    State(state): State<AppState>,
    SiteOrigin(origin): SiteOrigin,
    Query(q): Query<ListQuery>,
) -> Result<Json<CommentPage>> {
    let site = site::resolve(&state, &origin).await?;
    let thread = thread::get_or_create(&state, site.id, &q.path, None).await?;
    let page = comment::page(&state, thread.id, true, q.cursor, q.limit).await?;
    Ok(Json(CommentPage {
        total: thread.comment_count,
        comments: page.comments.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[derive(Debug, Deserialize)]
pub struct CreateComment {
    pub path: String,
    pub body_md: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub author_email: Option<String>,
    #[serde(default)]
    pub author_avatar: Option<String>,
    /// 已登录用户（宿主站自己签的 identity，未校验时只作展示）
    #[serde(default)]
    pub author_uid: Option<String>,
    #[serde(default)]
    pub challenge_token: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Created {
    pub id: Uuid,
    pub status: &'static str,
    pub body_html: String,
    pub created_at: DateTime<Utc>,
    /// 只有真正新插入才为 true；重放返回 false
    pub created: bool,
}

#[axum::debug_handler]
pub async fn create(
    State(state): State<AppState>,
    SiteOrigin(origin): SiteOrigin,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(input): Json<CreateComment>,
) -> Result<Json<Created>> {
    let site = site::resolve(&state, &origin).await?;
    if input.idempotency_key.as_ref().is_some_and(|k| k.len() > 64) {
        return Err(Problem::field("idempotency_key", "over 64 chars"));
    }

    let ip_key = ip
        .map(|v| v.to_string())
        .unwrap_or_else(|| "none".to_owned());

    antispam::hit(
        &state,
        &format!("ic:rl:c:{}", ip_key),
        antispam::COMMENT_BURST,
        60,
    )
    .await?;
    antispam::hit(
        &state,
        &format!("ic:rl:h:{}:{}", site.id, ip_key),
        antispam::COMMENT_HOURLY,
        3600,
    )
    .await?;

    let body_md = comment::validate_body(&input.body_md)?;
    let (spam, needs_review) = antispam::sniff(&body_md, input.author_name.as_deref());

    if antispam::challenge_needed(&site.challenge_mode, spam) {
        antispam::verify_challenge(&state, input.challenge_token.as_deref(), ip).await?;
    }

    let thread =
        thread::get_or_create(&state, site.id, &input.path, input.title.as_deref()).await?;
    let (parent_id, _depth) =
        comment::resolve_parent(&state, site.id, thread.id, input.parent_id).await?;

    let rendered = markdown::render(&body_md);
    let status: &'static str = if spam {
        "spam"
    } else if site.require_approval && (needs_review || input.author_uid.is_none()) {
        "pending"
    } else {
        "visible"
    };

    let (comment, created) = comment::insert(
        &state,
        comment::NewComment {
            site_id: site.id,
            thread_id: thread.id,
            parent_id,
            body_md: body_md.clone(),
            body_html: rendered.html.clone(),
            author_kind: "anon",
            author_uid: input.author_uid.clone(),
            author_name: input.author_name.clone(),
            author_avatar: input.author_avatar.clone(),
            author_email: input.author_email.clone(),
            ip_hash: antispam::ip_hash(&state, ip).await,
            ua_hash: headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(|ua| {
                    use sha2::Digest;
                    let mut h = sha2::Sha256::new();
                    h.update(ua.as_bytes());
                    format!("{:x}", h.finalize())[..16].to_owned()
                }),
            idempotency_key: input.idempotency_key.clone(),
            status,
            identity_unverified: input.author_uid.is_some(),
        },
    )
    .await?;

    if created {
        thread::bump(&state, thread.id, 1, comment.created_at).await?;
    }

    Ok(Json(Created {
        id: comment.id,
        status,
        body_html: rendered.html,
        created_at: comment.created_at,
        created,
    }))
}

/// 站点信息（widget footer 用）。
#[derive(Debug, Serialize)]
pub struct SiteMeta {
    pub site_id: Uuid,
    pub origin: String,
    pub name: Option<String>,
}

pub async fn site_meta(
    State(state): State<AppState>,
    SiteOrigin(origin): SiteOrigin,
) -> Result<Json<SiteMeta>> {
    let site = site::resolve(&state, &origin).await?;
    Ok(Json(SiteMeta {
        site_id: site.id,
        origin: site.origin,
        name: site.name,
    }))
}

/// 单条评论（深链落地）。
pub async fn one(
    State(state): State<AppState>,
    SiteOrigin(origin): SiteOrigin,
    Path(id): Path<Uuid>,
) -> Result<Json<CommentView>> {
    let site = site::resolve(&state, &origin).await?;
    let comment = comment::by_id(&state, site.id, id).await?;
    if comment.status == "deleted" {
        return Err(Problem::code(Code::CommentNotFound, "comment deleted"));
    }
    Ok(Json(comment.into()))
}
