//! 人数用的网页面：登录、建站向导、审核台、device 验证页。
//!
//! 会话用签名 cookie（HMAC-SHA256），不引入 session 存储。

use axum::{
    Form,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api, auth, claim,
    domain::{account, comment, moderation, site, thread},
    error::{Code, Problem, Result},
    oauth,
    state::AppState,
};

const COOKIE: &str = "ic_session";
const TTL_SECS: i64 = 7 * 86_400;

fn sign(secret: &str, payload: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(payload.as_bytes());
    hex(&mac.finalize().into_bytes())
}

/// 给别的模块签一段明文（投票人 cookie 用）。
pub fn sign_plain(state: &AppState, payload: &str) -> String {
    sign(session_secret(state), payload)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn session_secret(state: &AppState) -> &str {
    // prod 用 IC_SESSION_SECRET；dev 回落成按 base url 派生，重启不掉登录。
    &state.cfg().session_secret
}

fn issue_cookie(state: &AppState, account_id: Uuid) -> String {
    let expires = Utc::now().timestamp() + TTL_SECS;
    let payload = format!("{account_id}.{expires}");
    let sig = sign(session_secret(state), &payload);
    format!(
        "{COOKIE}={payload}.{sig}; Path=/; HttpOnly; SameSite=Lax; Max-Age={TTL_SECS}{}",
        if state.cfg().public_base_url.starts_with("https") {
            "; Secure"
        } else {
            ""
        }
    )
}

fn read_session(state: &AppState, headers: &HeaderMap) -> Option<Uuid> {
    let raw = headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{COOKIE}=")))?;

    let (payload, sig) = raw.rsplit_once('.')?;
    if sign(session_secret(state), payload) != sig {
        return None;
    }
    let mut fields = payload.split('.');
    let account_id: Uuid = fields.next()?.parse().ok()?;
    let expires: i64 = fields.next()?.parse().ok()?;
    (expires > Utc::now().timestamp()).then_some(account_id)
}

pub struct Session(pub account::Account);

impl axum::extract::FromRequestParts<AppState> for Session {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> std::result::Result<Self, Self::Rejection> {
        let Some(account_id) = read_session(state, &parts.headers) else {
            let target = parts.uri.path();
            return Err(Redirect::to(&format!("/login?next={target}")).into_response());
        };
        let account = account::by_id(state, account_id)
            .await
            .map_err(<Problem as IntoResponse>::into_response)?;
        Ok(Self(account))
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    pub error: Option<String>,
}

pub async fn login_page(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Html<String> {
    let cfg = state.cfg();
    let mut providers = String::new();
    for provider in oauth::available(cfg) {
        providers.push_str(&format!(
            r#"<a class="button" href="/auth/{id}">用 {label} 登录</a>"#,
            id = provider.as_str(),
            label = provider.label()
        ));
    }
    if providers.is_empty() && !cfg.email_login {
        providers.push_str(
            r#"<p class="error">没有可用的登录方式：管理员还没配 Google / GitHub OAuth。</p>"#,
        );
    }

    let email_form = if cfg.email_login {
        r##"<form method="post" action="/login" class="card">
  <h2>用邮箱进（开发用）</h2>
  <p class="muted">生产环境关掉，它不验证邮箱归属。</p>
  <label>邮箱<input name="email" type="email" required autocomplete="email"></label>
  <button>进入</button>
</form>"##
    } else {
        ""
    };

    let error = query
        .error
        .as_deref()
        .map(|msg| format!(r#"<p class="error">{}</p>"#, esc(msg)))
        .unwrap_or_default();

    Html(crate::web::render(
        "登录",
        &format!(
            r##"<div class="card">
  <h1>登录 iComment</h1>
  <p class="muted">账号只用来建站、审核和发令牌。评论者不需要登录。</p>
  {error}
  <div class="oauth">{providers}</div>
</div>
{email_form}"##
        ),
        false,
    ))
}

/// 跳去第三方授权页；state 落在短命 cookie 里，回来要能对上。
pub async fn oauth_start(
    State(state): State<AppState>,
    Path(raw): Path<String>,
) -> Result<Response> {
    let provider = oauth::Provider::parse(&raw)
        .ok_or_else(|| Problem::code(Code::NotFound, "unknown login provider"))?;
    let redirect_uri = oauth::callback_uri(state.cfg(), provider);
    let nonce = random_nonce();
    let target = oauth::authorize_url(state.cfg(), provider, &redirect_uri, &nonce)?;

    let mut res = Redirect::to(&target).into_response();
    set_cookie(
        &mut res,
        format!(
            "{}={nonce}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
            oauth::STATE_COOKIE,
            oauth::STATE_TTL_SECS,
            secure_suffix(&state)
        ),
    );
    Ok(res)
}

#[derive(Debug, Deserialize)]
pub struct OAuthQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

pub async fn oauth_callback(
    State(state): State<AppState>,
    Path(raw): Path<String>,
    Query(query): Query<OAuthQuery>,
    headers: HeaderMap,
) -> Result<Response> {
    let provider = oauth::Provider::parse(&raw)
        .ok_or_else(|| Problem::code(Code::NotFound, "unknown login provider"))?;

    let result = finish_login(&state, provider, &query, &headers).await;
    let mut res = match result {
        Ok(account) => {
            let mut res = Redirect::to("/dash").into_response();
            set_cookie(&mut res, issue_cookie(&state, account.id));
            res
        }
        Err(problem) => Redirect::to(&login_error_target(&problem)).into_response(),
    };
    // 不管成没成，state cookie 都作废
    set_cookie(
        &mut res,
        format!("{}=; Path=/; Max-Age=0", oauth::STATE_COOKIE),
    );
    Ok(res)
}

async fn finish_login(
    state: &AppState,
    provider: oauth::Provider,
    query: &OAuthQuery,
    headers: &HeaderMap,
) -> Result<account::Account> {
    if let Some(err) = &query.error {
        let detail = query
            .error_description
            .as_deref()
            .filter(|d| !d.is_empty())
            .unwrap_or(err);
        return Err(Problem::field("oauth", format!("{err}: {detail}")));
    }

    let expected = cookie_value(headers, oauth::STATE_COOKIE)
        .ok_or_else(|| Problem::field("state", "登录已过期或不是这个浏览器发起的，请重试"))?;
    if query.state.as_deref() != Some(expected.as_str()) {
        return Err(Problem::field("state", "state 不匹配，请重试"));
    }

    let code = query
        .code
        .as_deref()
        .filter(|c| !c.is_empty())
        .ok_or_else(|| Problem::field("code", "授权回调没带 code"))?;

    let redirect_uri = oauth::callback_uri(state.cfg(), provider);
    let profile = oauth::exchange_and_profile(state, provider, code, &redirect_uri).await?;
    account::login_with_identity(state, provider.as_str(), &profile.subject, &profile.email).await
}

fn login_error_target(problem: &Problem) -> String {
    let mut url = reqwest::Url::parse("https://login.local/login").expect("static url");
    url.query_pairs_mut()
        .append_pair("error", &problem.to_string());
    let mut out = url.path().to_owned();
    if let Some(q) = url.query() {
        out.push('?');
        out.push_str(q);
    }
    out
}

fn random_nonce() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn secure_suffix(state: &AppState) -> &'static str {
    if state.cfg().public_base_url.starts_with("https") {
        "; Secure"
    } else {
        ""
    }
}

fn set_cookie(res: &mut Response, value: String) {
    if let Ok(parsed) = value.parse() {
        res.headers_mut().insert(header::SET_COOKIE, parsed);
    }
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .map(str::to_owned)
}

/// 落地页：/ 就是这一页，讲清楚这是什么、怎么装、往哪进后台。
pub async fn landing(State(state): State<AppState>) -> Html<String> {
    let cfg = state.cfg();
    let base = &cfg.public_base_url;
    let snippet = api::agent::embed_snippet(&state, "<你的 site-key>");
    Html(crate::web::render(
        "自托管评论",
        &format!(
            r##"<section class="hero card">
  <h1>评论区长在你自己库里</h1>
  <p class="tagline">iComment 是开源自托管评论系统。Rust 单二进制，内嵌评论组件与迁移，
  数据只进你自己的 PostgreSQL，没有第三方脚本跟着访客跑。</p>
  <p class="cta">
    <a class="button" href="/dash">进入控制台</a>
    <a class="button ghost" href="/install.md">看安装说明</a>
  </p>
</section>

<div class="grid">
  <div class="card">
    <h2>agent 自己装站</h2>
    <p class="muted">一句 <code>claude mcp add</code> 加一次设备授权，agent 建站、贴 snippet、
    跑验收，人只在浏览器点一次确认。</p>
  </div>
  <div class="card">
    <h2>REST 是唯一权威</h2>
    <p class="muted">MCP 只是同权限的一层薄封装，删掉它服务照跑。任何客户端都能直接打
    <code>{base}/api</code>。</p>
  </div>
  <div class="card">
    <h2>12 KB 的影子 DOM</h2>
    <p class="muted">Lit 写的 Web Component，样式封在 Shadow DOM 里，不会被主题乱改，
    也不会改你的主题。</p>
  </div>
</div>

<div class="card">
  <h2>贴这两行就有评论区</h2>
  <pre>{snippet}</pre>
  <p class="muted">已开源 AGPL-3.0；<code>packages/widget</code>、<code>packages/sdk</code>、
  <code>packages/mcp</code> 是 MIT。</p>
</div>

<div class="card">
  <h2>给机器看的入口</h2>
  <ul class="list">
    <li class="row"><a href="/llms.txt">/llms.txt</a><span class="muted">一页说清接口与上手步骤</span></li>
    <li class="row"><a href="/install.md">/install.md</a><span class="muted">curl 版安装说明</span></li>
    <li class="row"><a href="/api/config">/api/config</a><span class="muted">公开配置探测</span></li>
  </ul>
</div>"##,
            base = esc(base),
            snippet = esc(&snippet),
        ),
        false,
    ))
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub email: String,
}

pub async fn login_submit(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Result<Response> {
    if !state.cfg().email_login {
        return Err(Problem::field(
            "email",
            "邮箱登录已关闭，请用 Google 或 GitHub 登录",
        ));
    }
    let account = account::ensure(&state, &form.email).await?;
    let cookie = issue_cookie(&state, account.id);
    let mut res = Redirect::to("/dash").into_response();
    res.headers_mut().insert(
        header::SET_COOKIE,
        cookie.parse().expect("cookie chars are ASCII"),
    );
    Ok(res)
}

pub async fn logout() -> Response {
    let mut res = Redirect::to("/login").into_response();
    res.headers_mut().insert(
        header::SET_COOKIE,
        format!("{COOKIE}=; Path=/; Max-Age=0").parse().unwrap(),
    );
    res
}

pub async fn dashboard(
    State(state): State<AppState>,
    Session(account): Session,
) -> Result<Html<String>> {
    let sites = site::list_for_owner(&state, account.id).await?;
    let mut rows = String::new();
    for site in &sites {
        rows.push_str(&format!(
            r#"<li class="row"><a href="/dash/sites/{id}">{origin}</a>
            <span class="muted">{n}</span></li>"#,
            id = site.id,
            origin = crate::web::esc(&site.origin),
            n = site
                .name
                .as_deref()
                .map(crate::web::esc)
                .unwrap_or_else(|| "未命名".to_owned())
        ));
    }
    if sites.is_empty() {
        rows.push_str("<li class=\"muted\">还没有站点</li>");
    }

    Ok(Html(crate::web::render(
        "控制台",
        &format!(
            r##"<div class="card">
  <div class="row"><h1>我的站点</h1><a href="/logout">退出</a></div>
  <ul class="list">{rows}</ul>
</div>
<form method="post" action="/dash/sites" class="card">
  <h2>新建站点</h2>
  <label>站点 origin<input name="origin" placeholder="example.com" required></label>
  <label>显示名（可选）<input name="name"></label>
  <button>创建</button>
</form>"##
        ),
        true,
    )))
}

#[derive(Deserialize)]
pub struct NewSiteForm {
    pub origin: String,
    pub name: Option<String>,
}

/// 建站第一步：开一个域名归属申请，去放 key 文件。
pub async fn create_site(
    State(state): State<AppState>,
    Session(account): Session,
    Form(form): Form<NewSiteForm>,
) -> Result<Response> {
    if state.cfg().claim_verify {
        let claim = claim::start(&state, account.id, &form.origin).await?;
        return Ok(Redirect::to(&format!("/dash/claims/{}", claim.id)).into_response());
    }
    let site = site::create(
        &state,
        account.id,
        site::CreateSite {
            origin: form.origin,
            name: form.name,
            require_approval: None,
            challenge_mode: None,
            blocked_domains: None,
        },
    )
    .await?;
    Ok(Redirect::to(&format!("/dash/sites/{}", site.id)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct ClaimForm {
    pub name: Option<String>,
}

/// 放文件的说明页：key、文件地址、验证按钮。
pub async fn claim_page(
    State(state): State<AppState>,
    Session(account): Session,
    Path(claim_id): Path<Uuid>,
) -> Result<Html<String>> {
    let claim = claim::by_id(&state, account.id, claim_id).await?;
    let file_url = claim.file_url();
    let host = reqwest::Url::parse(&claim.origin)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| claim.origin.clone());
    let expired = if claim.expired() {
        r#"<p class="error">这条申请已过期，回控制台重新申请一个。</p>"#
    } else {
        ""
    };

    Ok(Html(crate::web::render(
        "验证域名",
        &format!(
            r##"<div class="card">
  <h1>证明 {origin} 是你的</h1>
  <p class="muted">把下面这个文件放到站点根目录，能被公网匿名访问（不要登录、不要跳转），
  然后点验证。文件内容只有一行 key。</p>
  {expired}
  <p><code>{file_url}</code></p>
  <p>内容：<code>{key}</code></p>
  <p class="muted">也可以用 nginx 直接给：<code>location = /{key}.txt {{ default_type text/plain; return 200 "{key}"; }}</code></p>
  <form method="post" action="/dash/claims/{id}/verify">
    <label>显示名（可选）<input name="name" value="{host}" maxlength="64"></label>
    <button>验证并建站</button>
  </form>
</div>"##,
            origin = esc(&claim.origin),
            file_url = esc(&file_url),
            key = esc(&claim.claim_key),
            host = esc(&host),
            id = claim.id,
        ),
        true,
    )))
}

pub async fn claim_verify(
    State(state): State<AppState>,
    Session(account): Session,
    Path(claim_id): Path<Uuid>,
    Form(form): Form<ClaimForm>,
) -> Result<Response> {
    let claim = claim::by_id(&state, account.id, claim_id).await?;
    let (_, site) = claim::verify(
        &state,
        account.id,
        claim,
        site::CreateSite {
            origin: String::new(),
            name: form.name.filter(|n| !n.trim().is_empty()),
            require_approval: None,
            challenge_mode: None,
            blocked_domains: None,
        },
    )
    .await?;
    Ok(Redirect::to(&format!("/dash/sites/{}", site.id)).into_response())
}

struct SiteDetail {
    site: site::SiteView,
    threads: Vec<thread::Thread>,
    pending: i64,
    spam: i64,
    total: i64,
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub comment: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct VerifyQuery {
    pub verify: Option<String>,
    /// 一键签发可视化：填 uid 就在这里把 identity 打出来
    pub sign: Option<String>,
}

pub async fn site_page(
    State(state): State<AppState>,
    Session(account): Session,
    Path(site_id): Path<Uuid>,
    Query(query): Query<VerifyQuery>,
) -> Result<Html<String>> {
    let site = site::view(&state, site_id).await?;
    if !site::list_for_owner(&state, account.id)
        .await?
        .iter()
        .any(|s| s.id == site_id)
    {
        return Err(Problem::code(Code::Forbidden, "not your site"));
    }

    let threads: Vec<thread::Thread> = sqlx::query_as(
        "select * from thread where site_id = $1 order by last_at desc nulls last limit 30",
    )
    .bind(site_id)
    .fetch_all(state.db())
    .await?;
    let counts: (i64, i64, i64) = sqlx::query_as(
        "select \
         (select count(*) from comment where site_id = $1 and status = 'pending'), \
         (select count(*) from comment where site_id = $1 and status = 'spam'), \
         (select count(*) from comment where site_id = $1 and status <> 'deleted')",
    )
    .bind(site_id)
    .fetch_one(state.db())
    .await?;

    let detail = SiteDetail {
        site,
        threads,
        pending: counts.0,
        spam: counts.1,
        total: counts.2,
    };
    let verdict = match query.verify.as_deref() {
        None => None,
        Some("") => None,
        Some(url) => Some(api::verify::run(&state, &detail.site.site_key, url).await?),
    };
    let signed = query
        .sign
        .as_deref()
        .map(str::trim)
        .filter(|uid| !uid.is_empty())
        .map(|uid| {
            let header =
                crate::identity::issue(&detail.site.site_secret, &detail.site.site_key, uid, None);
            (uid.to_owned(), header)
        });
    Ok(Html(crate::web::render(
        "站点",
        &crate::web::site_detail(
            &state,
            &detail.site,
            &detail.threads,
            &SiteOps {
                pending: detail.pending,
                spam: detail.spam,
                total: detail.total,
                verdict: verdict.as_ref(),
                signed: signed.as_ref(),
            },
        ),
        true,
    )))
}

#[derive(Deserialize)]
pub struct ModerateForm {
    pub action: String,
    pub redirect: Option<String>,
}

pub async fn moderate(
    State(state): State<AppState>,
    Session(account): Session,
    Path((site_id, comment_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ModerateForm>,
) -> Result<Response> {
    let owned = site::list_for_owner(&state, account.id).await?;
    if !owned.iter().any(|s| s.id == site_id) {
        return Err(Problem::code(Code::Forbidden, "not your site"));
    }
    let action = match form.action.as_str() {
        "approve" => comment::Transition::Visible,
        "hide" => comment::Transition::Hidden,
        "spam" => comment::Transition::Spam,
        "delete" => comment::Transition::Deleted,
        other => return Err(Problem::field("action", format!("unknown {other}"))),
    };
    comment::transition(
        &state,
        site_id,
        comment_id,
        action,
        "user",
        Some(&account.id.to_string()),
        None,
    )
    .await?;
    let target = form
        .redirect
        .filter(|t| t.starts_with('/'))
        .unwrap_or_else(|| format!("/dash/sites/{site_id}"));
    Ok(Redirect::to(&target).into_response())
}

#[derive(Deserialize)]
pub struct SettingsForm {
    pub require_approval: Option<String>,
    pub challenge_mode: Option<String>,
    #[serde(default)]
    pub blocked_domains: Option<String>,
}

pub async fn save_settings(
    State(state): State<AppState>,
    Session(account): Session,
    Path(site_id): Path<Uuid>,
    Form(form): Form<SettingsForm>,
) -> Result<Response> {
    let owned = site::list_for_owner(&state, account.id).await?;
    if !owned.iter().any(|s| s.id == site_id) {
        return Err(Problem::code(Code::Forbidden, "not your site"));
    }
    site::update(
        &state,
        site_id,
        site::SiteUpdate {
            name: None,
            require_approval: Some(form.require_approval.as_deref() == Some("1")),
            challenge_mode: form.challenge_mode,
            blocked_domains: form.blocked_domains.map(|raw| {
                raw.split(['\n', ',', ' ', '\t', '\r'])
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .map(str::to_owned)
                    .collect()
            }),
        },
    )
    .await?;
    Ok(Redirect::to(&format!("/dash/sites/{site_id}")).into_response())
}

pub async fn thread_page(
    State(state): State<AppState>,
    Session(account): Session,
    Path((site_id, thread_id)): Path<(Uuid, Uuid)>,
) -> Result<Html<String>> {
    let owned = site::list_for_owner(&state, account.id).await?;
    if !owned.iter().any(|s| s.id == site_id) {
        return Err(Problem::code(Code::Forbidden, "not your site"));
    }
    let thread = thread::by_id(&state, site_id, thread_id).await?;
    let page = comment::page(&state, thread_id, true, None, 100).await?;
    Ok(Html(crate::web::render(
        "评论",
        &crate::web::thread_detail(&thread, &page.comments),
        true,
    )))
}

pub async fn device_page(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<DeviceQuery>,
) -> Result<Html<String>> {
    let prefill = q.code.unwrap_or_default();
    let _ = &state;
    Ok(Html(crate::web::render(
        "授权设备",
        &format!(
            r##"<form method="post" action="/device" class="card">
  <h1>授权 agent</h1>
  <p class="muted">把终端里显示的代码填进来。</p>
  <label>用户代码<input name="user_code" value="{}" placeholder="XXXX-XXXX" required></label>
  <button>授权</button>
</form>"##,
            crate::web::esc(&prefill)
        ),
        false,
    )))
}

#[derive(Deserialize)]
pub struct DeviceQuery {
    pub code: Option<String>,
}

#[derive(Deserialize)]
pub struct DeviceForm {
    pub user_code: String,
}

pub async fn device_submit(
    State(state): State<AppState>,
    Session(account): Session,
    Form(form): Form<DeviceForm>,
) -> Result<Html<String>> {
    auth::approve_device_code(state.db(), &form.user_code, account.id).await?;
    Ok(Html(crate::web::render(
        "已授权",
        r#"<div class="card"><h1>授权完成</h1><p>回到终端继续。</p></div>"#,
        false,
    )))
}

/// 令牌列表查询出来的原始行：(id, site_id, label, last_used_at)。
type TokenRow = (
    Uuid,
    Option<Uuid>,
    Option<String>,
    Option<chrono::DateTime<Utc>>,
);

pub async fn token_page(
    State(state): State<AppState>,
    Session(account): Session,
) -> Result<Html<String>> {
    let rows: Vec<TokenRow> = sqlx::query_as(
        "select id, site_id, label, last_used_at from agent_token \
             where account_id = $1 and revoked_at is null order by created_at desc",
    )
    .bind(account.id)
    .fetch_all(state.db())
    .await?;
    Ok(Html(crate::web::render(
        "令牌",
        &crate::web::token_list(&rows),
        true,
    )))
}

pub async fn not_found_web() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Html(crate::web::render(
            "404",
            "<div class=card><h1>没有这个页面</h1></div>",
            false,
        )),
    )
}

// ---------- 渲染片段 ----------

pub fn esc(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// 只允许我们自己渲染出的 HTML 通过（已做转义，这里不再二次处理）。
pub fn render(title: &str, body: &str, authed: bool) -> String {
    let nav = if authed {
        r#"<nav><a href="/dash">控制台</a><a href="/tokens">令牌</a><a href="/logout">退出</a></nav>"#
    } else {
        r#"<nav><a href="/">首页</a><a href="/login">登录</a></nav>"#
    };
    format!(
        r##"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} · iComment</title>
<link rel="stylesheet" href="/static/app.css">
</head><body>
<header><a class="brand" href="/">i<em>C</em>omment</a>{nav}</header>
<main>{body}</main>
<footer>自托管评论 · AGPL-3.0</footer>
</body></html>"##,
        title = esc(title),
    )
}

/// 验收结果渲染：ok 绿、partial 列出缺项与 hint、failed 只说原因。
fn verdict_html(result: &api::verify::VerifyResult) -> String {
    match &result.verdict {
        api::verify::Verdict::Ok { found } => format!(
            r#"<p class="ok">四项都齐（{found}），widget 已挂上。</p>"#,
            found = esc(&found.join("、")),
        ),
        api::verify::Verdict::Partial { missing, hint } => format!(
            r#"<p class="warn">抓到页面（HTTP {status}），但缺：{missing}</p><p class="muted">{hint}</p>"#,
            status = result.http_status.unwrap_or(0),
            missing = esc(&missing.join("、")),
            hint = esc(hint),
        ),
        api::verify::Verdict::Failed { reason, hint } => format!(
            r#"<p class="warn">没抓成：{reason}</p><p class="muted">{hint}</p>"#,
            reason = esc(reason),
            hint = esc(hint),
        ),
    }
}

/// 判数、验收结果、试签结果打包，避免参数爆炸。
#[derive(Default)]
pub struct SiteOps<'a> {
    pub pending: i64,
    pub spam: i64,
    pub total: i64,
    pub verdict: Option<&'a api::verify::VerifyResult>,
    pub signed: Option<&'a (String, String)>,
}

pub fn site_detail(
    state: &AppState,
    site: &site_web::SiteView,
    threads: &[thread_web::Thread],
    ops: &SiteOps<'_>,
) -> String {
    let SiteOps {
        pending,
        spam,
        total,
        verdict,
        signed,
    } = ops;
    let mut thread_rows = String::new();
    for t in threads {
        thread_rows.push_str(&format!(
            r#"<li class="row"><a href="/dash/sites/{sid}/threads/{tid}">{path}</a>
            <span class="muted">{count} 条</span></li>"#,
            sid = site.id,
            tid = t.id,
            path = esc(&t.path),
            count = t.comment_count
        ));
    }
    if threads.is_empty() {
        thread_rows.push_str("<li class=\"muted\">还没有页面收到评论</li>");
    }

    format!(
        r##"<div class="card">
  <h1>{origin}</h1>
  <p class="muted">{name}</p>
  <p>待审 <b>{pending}</b> · 垃圾 <b>{spam}</b> · 合计 <b>{total}</b></p>
  <p class="muted">关掉审核后新评论直接上线。</p>
  <form method="post" action="/dash/sites/{id}/settings" class="inline">
    <label class="check"><input type="checkbox" name="require_approval" value="1" {checked}> 需要审核</label>
    <label class="check">人机验证
      <select name="challenge_mode">
        <option value="suspicious" {sel_suspicious}>可疑时</option>
        <option value="always" {sel_always}>每条都要</option>
        <option value="off" {sel_off}>关</option>
      </select>
    </label>
    <label class="check">域名黑名单
      <input name="blocked_domains" value="{blocked}" placeholder="spam.example 每行一个">
    </label>
    <button>保存</button>
  </form>
  <p class="muted">黑名单命中即进垃圾箱，<code>example.com</code> 同时覆盖它的子域。</p>
  <p class="muted">{challenge_note}</p>
</div>
<div class="card"><h2>页面</h2><ul class="list">{thread_rows}</ul>
  <p><a href="/dash/sites/{id}/audit">看审核留痕 →</a></p></div>
<div class="card"><h2>嵌入代码</h2>
  <pre>{snippet}</pre>
</div>
<div class="card"><h2>透传身份</h2>
  <p class="muted">站点服务端用这把秘钥给已登录用户签 5 分钟有效的 <code>X-IC-Identity</code>。
  带上的评论免人机验证、免待审。秘钥别进前端。</p>
  <p class="muted">site_key <code>{site_key}</code></p>
  <pre>{secret}</pre>
  <form method="get" action="/dash/sites/{id}" class="inline">
    <input name="sign" placeholder="试签一个 uid" value="{sign_uid}">
    <button>签发</button>
  </form>
  {signed_block}
</div>
<div class="card"><h2>验收</h2>
  <p class="muted">贴上代码并部署后，填页面地址让服务端抓一次，检查四要素是否齐全。</p>
  <form method="get" action="/dash/sites/{id}" class="inline">
    <input name="verify" placeholder="{origin}/posts/hello" value="{verify_url}">
    <button>抓取验收</button>
  </form>
  {verdict}
</div>"##,
        origin = esc(&site.origin),
        name = site
            .name
            .as_deref()
            .map(esc)
            .unwrap_or_else(|| "未命名".to_owned()),
        id = site.id,
        verify_url = verdict.map(|v| esc(&v.url)).unwrap_or_default(),
        verdict = verdict.map(verdict_html).unwrap_or_default(),
        checked = if site.require_approval { "checked" } else { "" },
        sel_always = if site.challenge_mode == "always" {
            "selected"
        } else {
            ""
        },
        sel_suspicious = if site.challenge_mode == "suspicious" {
            "selected"
        } else {
            ""
        },
        sel_off = if site.challenge_mode == "off" {
            "selected"
        } else {
            ""
        },
        blocked = esc(&site.blocked_domains.join(" ")),
        challenge_note = if state.cfg().turnstile_required() {
            "Turnstile 已配置。"
        } else {
            "服务端没配 Turnstile 密钥，验证暂时不会生效。"
        },
        snippet = esc(&crate::api::agent::embed_snippet(state, &site.site_key)),
        site_key = esc(&site.site_key),
        secret = esc(&site.site_secret),
        sign_uid = signed.map(|(uid, _)| esc(uid)).unwrap_or_default(),
        signed_block = signed
            .map(|(uid, header)| format!(
                r#"<p>uid <code>{}</code> 的头：</p><pre>{}</pre>"#,
                esc(uid),
                esc(header)
            ))
            .unwrap_or_default(),
    )
}

/// 站点审核留痕。`only` 有值时只看这一条评论的历史。
pub fn audit_view(events: &[moderation::Event], only: Option<Uuid>) -> String {
    let mut rows = String::new();
    for e in events {
        let target = match (&e.thread_path, e.comment_id) {
            (Some(path), Some(id)) => format!(
                r#"<a href="/dash/comments?focus={id}"><code>{}</code></a>"#,
                esc(path)
            ),
            (Some(path), None) => format!("<code>{}</code>", esc(path)),
            _ => "<span class=\"muted\">（已移除）</span>".to_owned(),
        };
        rows.push_str(&format!(
            r##"<li>
  <div class="meta"><b>{who}</b> · {when} · <span class="pill">{act}</span></div>
  <div class="muted">对 {target}{reason}</div>
</li>"##,
            who = moderation::actor(&e.actor_kind),
            when = e.created_at.format("%m-%d %H:%M:%S"),
            act = moderation::label(&e.action),
            reason = e
                .reason
                .as_deref()
                .map(|r| format!("：{}", esc(r)))
                .unwrap_or_default(),
        ));
    }
    if rows.is_empty() {
        rows.push_str("<li class=\"muted\">还没有审核动作</li>");
    }
    let head = match only {
        Some(id) => format!(
            r#"<h1>这条评论的历史</h1><p class="muted">{id}</p>"#,
            id = id
        ),
        None => {
            "<h1>审核留痕</h1><p class=\"muted\">最近的放行、隐藏、删除都记在这。</p>".to_owned()
        }
    };
    format!(
        r#"<div class="card">{head}</div><div class="card"><ul class="comments">{rows}</ul></div>"#
    )
}

pub async fn audit_page(
    State(state): State<AppState>,
    Session(account): Session,
    Path(site_id): Path<Uuid>,
    Query(q): Query<AuditQuery>,
) -> Result<Html<String>> {
    let owned = site::list_for_owner(&state, account.id).await?;
    if !owned.iter().any(|s| s.id == site_id) {
        return Err(Problem::code(Code::Forbidden, "not your site"));
    }
    let (events, only) = match q.comment {
        Some(comment_id) => (
            moderation::for_comment(&state, site_id, comment_id).await?,
            Some(comment_id),
        ),
        None => (moderation::recent(&state, site_id, 200).await?, None),
    };
    Ok(Html(render("审核留痕", &audit_view(&events, only), true)))
}

pub fn thread_detail(thread: &thread_web::Thread, comments: &[comment_web::Comment]) -> String {
    let mut rows = String::new();
    for c in comments {
        let actions = format!(
            r#"<form method="post" action="/dash/comments/{id}/moderate" class="inline">
            <input type="hidden" name="redirect" value="/dash/sites/{sid}/threads/{tid}">
            <button name="action" value="approve">通过</button>
            <button name="action" value="hide">隐藏</button>
            <button name="action" value="spam">垃圾</button>
            <button name="action" value="delete">删除</button></form>"#,
            id = c.id,
            sid = thread.site_id,
            tid = thread.id,
        );
        rows.push_str(&format!(
            r##"<li class="comment {status}">
  <div class="meta"><b>{author}</b> · {when} · <code>{status}</code></div>
  <div class="body">{html}</div>
  {actions}
</li>"##,
            status = esc(&c.status),
            author = esc(c.author_name.as_deref().unwrap_or("匿名")),
            when = c.created_at.format("%m-%d %H:%M"),
            html = c.body_html,
        ));
    }
    if rows.is_empty() {
        rows.push_str("<li class=\"muted\">这个页面还没有评论</li>");
    }
    format!(
        r#"<div class="card"><h1>{path}</h1><p class="muted">{count} 条评论</p></div>
<div class="card"><ul class="comments">{rows}</ul></div>"#,
        path = esc(&thread.path),
        count = thread.comment_count,
    )
}

pub async fn revoke_token(
    State(state): State<AppState>,
    session: Session,
    Path(token_id): Path<Uuid>,
) -> Result<Response> {
    sqlx::query(
        "update agent_token set revoked_at = now() \
         where id = $1 and account_id = $2 and revoked_at is null",
    )
    .bind(token_id)
    .bind(session.0.id)
    .execute(state.db())
    .await?;
    Ok(Redirect::to("/tokens").into_response())
}

pub fn token_list(rows: &[TokenRow]) -> String {
    let mut out = String::new();
    for (id, site_id, label, last_used) in rows {
        out.push_str(&format!(
            r##"<li class="row"><code>{short}</code><span class="muted">{scope} · {last}</span>
  <form method="post" action="/tokens/{id}/revoke"><button type="submit">撤销</button></form></li>"##,
            short = &id.to_string()[..8],
            scope = if site_id.is_some() { "站点" } else { "账号" },
            last = last_used
                .map(|t| format!("最后使用 {}", t.format("%Y-%m-%d %H:%M")))
                .unwrap_or_else(|| "未使用".to_owned()),
        ));
        let _ = label;
    }
    if out.is_empty() {
        out.push_str("<li class=\"muted\">没有有效令牌</li>");
    }
    format!(
        r#"<div class="card"><h1>令牌</h1>
<p class="muted">明文只在签发时出现，这里只显示前缀。用 agent 走 device flow 拿新令牌。</p>
<ul class="list">{out}</ul></div>"#
    )
}

// 别名，避免和 handler 里的 site/thread/comment 模块名混
use crate::domain::{comment as comment_web, site as site_web, thread as thread_web};
