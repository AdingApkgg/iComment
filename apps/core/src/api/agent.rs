//! agent 面 API：device flow + 建站 + 管评论。
//!
//! 账号级 token 才能建站；站点级 token 只能动绑定的那个站。

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{self, Agent, DeviceOutcome},
    domain::{account, comment, site, thread},
    error::{Code, Problem, Result},
    markdown,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
    pub interval: i64,
}

pub async fn device_start(State(state): State<AppState>) -> Result<Json<DeviceStart>> {
    let flow = auth::start_device_flow(state.db()).await?;
    let base = &state.cfg().public_base_url;
    Ok(Json(DeviceStart {
        device_code: flow.device_code,
        user_code: flow.user_code.clone(),
        verification_uri: format!("{base}/device"),
        verification_uri_complete: format!("{base}/device?code={}", flow.user_code),
        expires_in: flow.expires_in,
        interval: 5,
    }))
}

#[derive(Debug, Deserialize)]
pub struct DevicePoll {
    pub device_code: String,
}

#[derive(Debug, Serialize)]
pub struct TokenIssued {
    pub token: String,
    pub account_id: Uuid,
    pub scope: &'static str,
}

pub async fn device_token(
    State(state): State<AppState>,
    Json(input): Json<DevicePoll>,
) -> Result<Json<TokenIssued>> {
    match auth::poll_device_token(state.db(), &input.device_code).await? {
        DeviceOutcome::Pending => Err(Problem::code(
            Code::DevicePending,
            "waiting for user approval",
        )),
        DeviceOutcome::Expired => Err(Problem::code(Code::DeviceExpired, "device_code expired")),
        DeviceOutcome::Approved { token, account_id } => Ok(Json(TokenIssued {
            token,
            account_id,
            scope: "account",
        })),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateSiteInput {
    pub origin: String,
    #[serde(default)]
    pub name: Option<String>,
    /// 建站时顺手定下审核/验证策略，省一次 PATCH。
    #[serde(default)]
    pub require_approval: Option<bool>,
    #[serde(default)]
    pub challenge_mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SiteCreated {
    #[serde(flatten)]
    pub site: site::SiteView,
    /// 建站即发一个站点级 token，agent 后续用它，避免每次带账号 token
    pub site_token: String,
    pub embed_snippet: String,
}

pub async fn create_site(
    State(state): State<AppState>,
    agent: Agent,
    Json(input): Json<CreateSiteInput>,
) -> Result<Json<SiteCreated>> {
    if agent.site_id.is_some() {
        return Err(Problem::code(
            Code::Forbidden,
            "site-scoped token cannot create sites",
        ));
    }

    let site = site::create(
        &state,
        agent.account_id,
        site::CreateSite {
            origin: input.origin,
            name: input.name,
            require_approval: input.require_approval,
            challenge_mode: input.challenge_mode,
        },
    )
    .await?;
    let view = site::view(&state, site.id).await?;
    let site_token = auth::issue_site_token(state.db(), agent.account_id, site.id, "site").await?;
    let embed_snippet = embed_snippet(&state, &view.site_key);

    Ok(Json(SiteCreated {
        site: view,
        site_token,
        embed_snippet,
    }))
}

pub fn embed_snippet(state: &AppState, site_key: &str) -> String {
    format!(
        "<script src=\"{}/icomment.js?v={version}\" data-site-key=\"{site_key}\" defer></script>\n\
         <noscript>评论区需要 JavaScript</noscript>",
        state.cfg().widget_cdn,
        version = env!("CARGO_PKG_VERSION"),
        site_key = site_key
    )
}

#[derive(Debug, Serialize)]
pub struct SiteList {
    pub sites: Vec<site::SiteView>,
}

pub async fn list_sites(State(state): State<AppState>, agent: Agent) -> Result<Json<SiteList>> {
    if let Some(site_id) = agent.site_id {
        let site = site::view(&state, site_id).await?;
        return Ok(Json(SiteList { sites: vec![site] }));
    }
    Ok(Json(SiteList {
        sites: site::list_for_owner(&state, agent.account_id).await?,
    }))
}

fn ensure_site_scope(agent: &Agent, site_id: Uuid) -> Result<()> {
    if agent.allows_site(site_id) {
        Ok(())
    } else {
        Err(Problem::code(
            Code::Forbidden,
            "token bound to another site",
        ))
    }
}

pub async fn get_site(
    State(state): State<AppState>,
    agent: Agent,
    Path(site_id): Path<Uuid>,
) -> Result<Json<site::SiteView>> {
    ensure_site_scope(&agent, site_id)?;
    Ok(Json(site::view(&state, site_id).await?))
}

#[derive(Debug, Deserialize)]
pub struct SitePatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub require_approval: Option<bool>,
    #[serde(default)]
    pub challenge_mode: Option<String>,
}

pub async fn patch_site(
    State(state): State<AppState>,
    agent: Agent,
    Path(site_id): Path<Uuid>,
    Json(patch): Json<SitePatch>,
) -> Result<Json<site::SiteView>> {
    ensure_site_scope(&agent, site_id)?;
    Ok(Json(
        site::update(
            &state,
            site_id,
            site::SiteUpdate {
                name: patch.name,
                require_approval: patch.require_approval,
                challenge_mode: patch.challenge_mode,
            },
        )
        .await?,
    ))
}

pub async fn delete_site(
    State(state): State<AppState>,
    agent: Agent,
    Path(site_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    ensure_site_scope(&agent, site_id)?;
    site::soft_delete(&state, site_id).await?;
    Ok(Json(serde_json::json!({ "deleted": site_id })))
}

#[derive(Debug, Deserialize)]
pub struct CommentQuery {
    pub path: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub cursor: Option<DateTime<Utc>>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
pub struct AdminComment {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub body_md: String,
    pub status: String,
    pub author_name: Option<String>,
    pub author_email: Option<String>,
    pub identity_unverified: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AdminPage {
    pub thread_id: Uuid,
    pub path: String,
    pub comments: Vec<AdminComment>,
    pub next_cursor: Option<DateTime<Utc>>,
}

/// 审核台读取：包含全部状态，可按状态过滤。
pub async fn list_comments(
    State(state): State<AppState>,
    agent: Agent,
    Path(site_id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<CommentQuery>,
) -> Result<Json<AdminPage>> {
    ensure_site_scope(&agent, site_id)?;
    let thread = thread::get_or_create(&state, site_id, &q.path, None).await?;

    let rows: Vec<comment::Comment> = sqlx::query_as(
        "select id, thread_id, parent_id, body_md, body_html, author_kind, author_uid, \
         author_name, author_avatar, status, identity_unverified, created_at from comment \
         where thread_id = $1 and ($2::text is null or status = $2) \
         and ($3::timestamptz is null or created_at < $3) \
         order by created_at desc limit $4",
    )
    .bind(thread.id)
    .bind(q.status.as_deref())
    .bind(q.cursor)
    .bind(q.limit.clamp(1, 100))
    .fetch_all(state.db())
    .await?;

    let next_cursor = rows.last().map(|c| c.created_at);
    Ok(Json(AdminPage {
        thread_id: thread.id,
        path: thread.path,
        comments: rows
            .into_iter()
            .map(|c| AdminComment {
                id: c.id,
                parent_id: c.parent_id,
                body_md: c.body_md,
                status: c.status,
                author_name: c.author_name,
                author_email: c.author_email,
                identity_unverified: c.identity_unverified,
                created_at: c.created_at,
            })
            .collect(),
        next_cursor,
    }))
}

#[derive(Debug, Deserialize)]
pub struct CreateOwn {
    pub path: String,
    pub body_md: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

/// 站长用 agent 发正式回复（直接 visible、author_kind=owner）。
pub async fn create_comment(
    State(state): State<AppState>,
    agent: Agent,
    Path(site_id): Path<Uuid>,
    Json(input): Json<CreateOwn>,
) -> Result<Json<comment::Comment>> {
    ensure_site_scope(&agent, site_id)?;
    let body_md = comment::validate_body(&input.body_md)?;
    let thread = thread::get_or_create(&state, site_id, &input.path, None).await?;
    let (parent_id, _) =
        comment::resolve_parent(&state, site_id, thread.id, input.parent_id).await?;

    let (comment, created) = comment::insert(
        &state,
        comment::NewComment {
            site_id,
            thread_id: thread.id,
            parent_id,
            body_md: body_md.clone(),
            body_html: markdown::render(&body_md).html,
            author_kind: "owner",
            author_uid: Some(agent.account_id.to_string()),
            author_name: Some("站长".to_owned()),
            author_avatar: None,
            author_email: None,
            ip_hash: None,
            ua_hash: None,
            idempotency_key: input.idempotency_key,
            status: "visible",
            identity_unverified: false,
        },
    )
    .await?;
    if created {
        thread::bump(&state, thread.id, 1, comment.created_at).await?;
    }
    Ok(Json(comment))
}

#[derive(Debug, Deserialize)]
pub struct Moderate {
    pub action: String,
    #[serde(default)]
    pub reason: Option<String>,
}

pub async fn moderate(
    State(state): State<AppState>,
    agent: Agent,
    Path((site_id, comment_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<Moderate>,
) -> Result<Json<comment::Comment>> {
    ensure_site_scope(&agent, site_id)?;
    let action = match input.action.as_str() {
        "approve" | "visible" => comment::Transition::Visible,
        "hide" => comment::Transition::Hidden,
        "spam" => comment::Transition::Spam,
        "delete" => comment::Transition::Deleted,
        other => return Err(Problem::field("action", format!("unknown action {other}"))),
    };
    Ok(Json(
        comment::transition(
            &state,
            site_id,
            comment_id,
            action,
            "agent",
            Some(&agent.account_id.to_string()),
            input.reason.as_deref(),
        )
        .await?,
    ))
}

#[derive(Debug, Serialize)]
pub struct ThreadList {
    pub threads: Vec<thread::Thread>,
}

pub async fn list_threads(
    State(state): State<AppState>,
    agent: Agent,
    Path(site_id): Path<Uuid>,
) -> Result<Json<ThreadList>> {
    ensure_site_scope(&agent, site_id)?;
    let threads: Vec<thread::Thread> = sqlx::query_as(
        "select * from thread where site_id = $1 order by last_at desc nulls last limit 100",
    )
    .bind(site_id)
    .fetch_all(state.db())
    .await?;
    Ok(Json(ThreadList { threads }))
}

#[derive(Debug, Serialize)]
pub struct Me {
    pub account_id: Uuid,
    pub email: String,
    pub site_id: Option<Uuid>,
    pub scope: &'static str,
}

pub async fn me(State(state): State<AppState>, agent: Agent) -> Result<Json<Me>> {
    let account = account::by_id(&state, agent.account_id).await?;
    Ok(Json(Me {
        account_id: agent.account_id,
        email: account.email,
        site_id: agent.site_id,
        scope: if agent.site_id.is_some() {
            "site"
        } else {
            "account"
        },
    }))
}

#[derive(Debug, Deserialize)]
pub struct RevokeInput {
    /// 省略时撤销调用所用的这个 token。
    #[serde(default)]
    pub token_id: Option<Uuid>,
}

pub async fn revoke_token(
    State(state): State<AppState>,
    agent: Agent,
    Json(input): Json<RevokeInput>,
) -> Result<Json<serde_json::Value>> {
    let target = input.token_id.unwrap_or(agent.token_id);
    let affected = sqlx::query(
        "update agent_token set revoked_at = now() \
         where id = $1 and account_id = $2 and revoked_at is null",
    )
    .bind(target)
    .bind(agent.account_id)
    .execute(state.db())
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(Problem::code(Code::Validation, "no such active token"));
    }
    Ok(Json(serde_json::json!({ "revoked": target })))
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub threads: i64,
    pub comments: i64,
    pub pending: i64,
    pub spam: i64,
}

pub async fn stats(
    State(state): State<AppState>,
    agent: Agent,
    Path(site_id): Path<Uuid>,
) -> Result<Json<Stats>> {
    ensure_site_scope(&agent, site_id)?;
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        "select \
         (select count(*) from thread where site_id = $1), \
         (select count(*) from comment where site_id = $1 and status <> 'deleted'), \
         (select count(*) from comment where site_id = $1 and status = 'pending'), \
         (select count(*) from comment where site_id = $1 and status = 'spam')",
    )
    .bind(site_id)
    .fetch_one(state.db())
    .await?;
    Ok(Json(Stats {
        threads: row.0,
        comments: row.1,
        pending: row.2,
        spam: row.3,
    }))
}
