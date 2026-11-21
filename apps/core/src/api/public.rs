//! 公开 API：widget 读取与写评论。鉴权靠 Origin，不需要 token。

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    akismet, antispam,
    domain::{comment, site, thread},
    error::{Code, Problem, Result},
    extract::{ClientIp, SiteOrigin},
    identity, markdown, reaction,
    state::AppState,
    voter,
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
    pub reactions: reaction::Tally,
}

fn view(c: comment::Comment, reactions: reaction::Tally) -> CommentView {
    CommentView {
        id: c.id,
        parent_id: c.parent_id,
        body_html: c.body_html,
        author_kind: c.author_kind,
        author_name: c.author_name,
        author_avatar: c.author_avatar,
        status: c.status,
        identity_unverified: c.identity_unverified,
        created_at: c.created_at,
        reactions,
    }
}

/// 一页评论的反应票数一次查完，避免 N+1。
async fn views(
    state: &AppState,
    rows: Vec<comment::Comment>,
    voter: Option<&str>,
) -> Result<Vec<CommentView>> {
    let ids: Vec<Uuid> = rows.iter().map(|c| c.id).collect();
    let tallies = reaction::batch(state, &ids, voter).await?;
    Ok(rows
        .into_iter()
        .map(|c| {
            let tally = tallies.get(&c.id).cloned().unwrap_or_default();
            view(c, tally)
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct ReactionPath {
    pub id: Uuid,
    pub kind: String,
}

/// 投票；`DELETE` 是撤票。可带 `?voter=` 覆盖 cookie 身份。
#[axum::debug_handler]
pub async fn react(
    State(state): State<AppState>,
    SiteOrigin(origin): SiteOrigin,
    ClientIp(ip): ClientIp,
    Path(ReactionPath { id, kind }): Path<ReactionPath>,
    headers: HeaderMap,
) -> Result<Response> {
    let site = site::resolve(&state, &origin).await?;
    antispam::hit(
        &state,
        &antispam::reaction_key(site.id, ip),
        antispam::REACTION_BURST,
        60,
    )
    .await?;
    let voter = match headers
        .get("x-ic-voter")
        .and_then(|v| v.to_str().ok())
        .map(voter::clean)
    {
        Some(Ok(v)) => (v, None),
        Some(Err(e)) => return Err(e),
        None => voter::resolve(&headers, &state),
    };
    let tally = reaction::add(&state, site.id, id, &kind, &voter.0).await?;
    drop_cookie(tally, voter.1)
}

#[axum::debug_handler]
pub async fn unreact(
    State(state): State<AppState>,
    SiteOrigin(origin): SiteOrigin,
    ClientIp(ip): ClientIp,
    Path(ReactionPath { id, kind }): Path<ReactionPath>,
    headers: HeaderMap,
) -> Result<Response> {
    let site = site::resolve(&state, &origin).await?;
    antispam::hit(
        &state,
        &antispam::reaction_key(site.id, ip),
        antispam::REACTION_BURST,
        60,
    )
    .await?;
    let voter = match headers
        .get("x-ic-voter")
        .and_then(|v| v.to_str().ok())
        .map(voter::clean)
    {
        Some(Ok(v)) => (v, None),
        Some(Err(e)) => return Err(e),
        None => voter::resolve(&headers, &state),
    };
    let tally = reaction::remove(&state, site.id, id, &kind, &voter.0).await?;
    drop_cookie(tally, voter.1)
}

fn drop_cookie(tally: reaction::Tally, cookie: Option<String>) -> Result<Response> {
    let mut res = Json(tally).into_response();
    if let Some(cookie) = cookie {
        res.headers_mut()
            .insert(header::SET_COOKIE, cookie.parse().expect("ascii"));
    }
    Ok(res)
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
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Response> {
    let site = site::resolve(&state, &origin).await?;
    // 拿到站点秘钥才能读 secret 列——这里不影响公开面
    let thread = thread::get_or_create(&state, site.id, &q.path, None).await?;
    let page = comment::page(&state, thread.id, true, q.cursor, q.limit).await?;
    let (who, cookie) = voter::resolve(&headers, &state);
    let listed = views(&state, page.comments, Some(&who)).await?;
    let mut res = Json(CommentPage {
        total: thread.comment_count,
        comments: listed,
        next_cursor: page.next_cursor,
    })
    .into_response();
    if let Some(cookie) = cookie {
        res.headers_mut()
            .insert(header::SET_COOKIE, cookie.parse().expect("ascii"));
    }
    Ok(res)
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
    Json(mut input): Json<CreateComment>,
) -> Result<Json<Created>> {
    // 头优先：调用方重试时换不出新 body，但能原样再发一次同一把钥匙
    if input.idempotency_key.is_none()
        && let Some(key) = headers
            .get("idempotency-key")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty())
    {
        input.idempotency_key = Some(key.to_owned());
    }
    let site = site::resolve(&state, &origin).await?;
    Ok(Json(
        submit(&state, &site, ip, &headers, input, false).await?,
    ))
}

/// 写入一条匿名评论。JSON 面与无 JS 的 embed 表单共用，后者跳过人机验证。
pub async fn submit(
    state: &AppState,
    site: &site::Site,
    ip: Option<std::net::IpAddr>,
    headers: &HeaderMap,
    input: CreateComment,
    skip_challenge: bool,
) -> Result<Created> {
    if input.idempotency_key.as_ref().is_some_and(|k| k.len() > 64) {
        return Err(Problem::field("idempotency_key", "over 64 chars"));
    }

    let ip_key = ip
        .map(|v| v.to_string())
        .unwrap_or_else(|| "none".to_owned());

    antispam::hit(
        state,
        &format!("ic:rl:c:{}", ip_key),
        antispam::COMMENT_BURST,
        60,
    )
    .await?;
    antispam::hit(
        state,
        &format!("ic:rl:h:{}:{}", site.id, ip_key),
        antispam::COMMENT_HOURLY,
        3600,
    )
    .await?;

    // 单站日写入软顶：超了只强制进队列，不拒
    let daily = antispam::count(state, &format!("ic:daily:{}", site.id), 86_400)
        .await
        .unwrap_or(0);
    let soft_capped = daily > antispam::DAILY_SOFT_CAP;

    let body_md = comment::validate_body(&input.body_md)?;
    let (spam, needs_review) = antispam::sniff(
        &body_md,
        input.author_name.as_deref(),
        &site.blocked_domains,
    );

    // 宿主站透传的身份：验过就采信签名里的值（免审核、免挑战），没验过只当展示
    let identity = identity::read(site, headers);
    let verified = matches!(&identity, Some((_, true)));

    // 本地规则已放行、又还没验证过身份时，再问一次 Akismet；失败按本地结论走
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok());
    let spam = if spam || verified {
        spam
    } else {
        akismet::says_spam(
            state,
            &site.origin,
            ip.map(|v| v.to_string()).as_deref(),
            ua,
            &body_md,
            input.author_name.as_deref(),
            input.author_email.as_deref(),
        )
        .await
        .unwrap_or(false)
    };

    if !skip_challenge && !verified && antispam::challenge_needed(&site.challenge_mode, spam) {
        antispam::verify_challenge(state, input.challenge_token.as_deref(), ip).await?;
    }

    let thread = thread::get_or_create(state, site.id, &input.path, input.title.as_deref()).await?;
    let (parent_id, _depth) =
        comment::resolve_parent(state, site.id, thread.id, input.parent_id).await?;

    let (author_uid, author_name, author_avatar) = match &identity {
        Some((id, true)) => (
            Some(id.uid.clone()),
            input.author_name.clone().or_else(|| id.name.clone()),
            input.author_avatar.clone().or_else(|| id.avatar.clone()),
        ),
        _ => (
            input.author_uid.clone(),
            input.author_name.clone(),
            input.author_avatar.clone(),
        ),
    };

    let rendered = markdown::render(&body_md);
    let status: &'static str = if spam {
        "spam"
    } else if soft_capped || (site.require_approval && (needs_review || !verified)) {
        "pending"
    } else {
        "visible"
    };

    let (comment, created) = comment::insert(
        state,
        comment::NewComment {
            site_id: site.id,
            thread_id: thread.id,
            parent_id,
            body_md: body_md.clone(),
            body_html: rendered.html.clone(),
            author_kind: "anon",
            author_uid,
            author_name,
            author_avatar,
            author_email: input.author_email.clone(),
            ip_hash: antispam::ip_hash(state, ip).await,
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
            identity_unverified: identity.is_some() && !verified,
        },
    )
    .await?;

    if created {
        thread::bump(state, thread.id, 1, comment.created_at).await?;
    }

    Ok(Created {
        id: comment.id,
        status,
        body_html: rendered.html,
        created_at: comment.created_at,
        created,
    })
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
    headers: HeaderMap,
) -> Result<Json<CommentView>> {
    let site = site::resolve(&state, &origin).await?;
    let comment = comment::by_id(&state, site.id, id).await?;
    if comment.status == "deleted" {
        return Err(Problem::code(Code::CommentNotFound, "comment deleted"));
    }
    let (who, _) = voter::resolve(&headers, &state);
    let tally = reaction::tally(&state, id, Some(&who)).await?;
    Ok(Json(view(comment, tally)))
}
