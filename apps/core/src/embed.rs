//! 无 JS 兜底页：`/embed/{site_key}{path}`。
//!
//! widget 加载失败、脚本被 CSP 拦、或访客直接关掉 JS 时，这条链接是最后能读能写的地方。
//! 全部服务端渲染，表单是原生 POST，提交完 303 回原页。

use axum::{
    Form,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use crate::{
    api::public::{self, CreateComment, Created},
    domain::{comment, site, thread},
    error::{Problem, Result},
    extract::ClientIp,
    normalize,
    state::AppState,
    web,
};

#[derive(Debug, Deserialize)]
pub struct Target {
    pub site_key: String,
    /// 通配段不带前导斜杠；直接挂在站根时为空。
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Keys {
    /// 深链用：只渲染这一条及其子回复
    #[serde(default)]
    pub focus: Option<uuid::Uuid>,
    /// 提交成功后回到这里
    #[serde(default)]
    pub sent: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// 通配段 → 归一化路径。
fn page_path(raw: Option<&str>) -> Result<String> {
    let joined = format!("/{}", raw.unwrap_or("").trim_matches('/'));
    normalize::normalize_path(&joined)
        .ok_or_else(|| Problem::field("path", "unusable path for a comment thread"))
}

async fn page(state: &AppState, target: &Target) -> Result<(site::Site, thread::Thread)> {
    let site = site::by_key(state, &target.site_key).await?;
    let path = page_path(target.path.as_deref())?;
    let thread = thread::get_or_create(state, site.id, &path, None).await?;
    Ok((site, thread))
}

pub async fn show(
    State(state): State<AppState>,
    Path(target): Path<Target>,
    Query(keys): Query<Keys>,
) -> Result<Response> {
    let (site, thread) = page(&state, &target).await?;
    let mut list = comment::page(&state, thread.id, true, None, 200)
        .await?
        .comments;
    let total = comment::count(&state, thread.id).await?;
    if let Some(focus) = keys.focus {
        let anchor = comment::by_id(&state, site.id, focus).await?;
        let children = comment::children(&state, thread.id, focus).await?;
        list = std::iter::once(anchor).chain(children).collect();
    }

    let mut rows = String::new();
    for c in &list {
        rows.push_str(&row_html(c));
    }
    if rows.is_empty() {
        rows.push_str("<li class=\"muted\">还没有评论，来当第一个。</li>");
    }

    let notice = match (keys.sent.is_some(), keys.error.as_deref()) {
        (true, _) => "<p class=\"ok\">已收到，稍后刷新可见。</p>".to_owned(),
        (_, Some(err)) => format!("<p class=\"warn\">{}</p>", web::esc(err)),
        _ => String::new(),
    };

    let action = format!(
        "/embed/{}{}",
        web::esc(&site.site_key),
        web::esc(&page_path(target.path.as_deref())?)
    );
    let title = if thread.path == "/" {
        site.name.clone().unwrap_or_else(|| site.origin.clone())
    } else {
        thread.path.clone()
    };

    let body = format!(
        r#"<div class="card">
  <h1>{title}</h1>
  <p class="muted">{origin} · 共 {total} 条</p>
  {notice}
</div>
<div class="card"><ul class="comments">{rows}</ul></div>
<div class="card">
  <h2>写一条</h2>
  <p class="muted">这个页面不需要 JavaScript。</p>
  <form method="post" action="{action}" class="stack">
    <input type="hidden" name="path" value="{path}">
    <label>昵称<input name="author_name" autocomplete="nickname" maxlength="40"></label>
    <label>邮箱（可选，不公开）<input name="author_email" type="email" autocomplete="email"></label>
    <label>内容<textarea name="body_md" required placeholder="支持 Markdown"></textarea></label>
    <button>发表</button>
  </form>
</div>"#,
        title = web::esc(&title),
        origin = web::esc(&site.origin),
        total = total,
        path = web::esc(&thread.path),
    );

    Ok(Html(web::render(&title, &body, false)).into_response())
}

fn row_html(c: &comment::Comment) -> String {
    format!(
        r##"<li class="comment {status}">
  <div class="meta"><b>{author}</b> · {when} · <code>{status}</code></div>
  <div class="body">{html}</div>
</li>"##,
        status = web::esc(&c.status),
        author = web::esc(c.author_name.as_deref().unwrap_or("匿名")),
        when = c.created_at.format("%m-%d %H:%M"),
        html = c.body_html,
    )
}

#[derive(Debug, Deserialize)]
pub struct FormInput {
    pub path: String,
    #[serde(default)]
    pub body_md: String,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub author_email: Option<String>,
}

/// 原生表单提交：没有 Turnstile 令牌可交，所以这里跳过人机验证
/// （表单本身就是「真人手敲」的弱证明，其余反垃圾照旧）。
#[axum::debug_handler]
pub async fn submit(
    State(state): State<AppState>,
    Path(target): Path<Target>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Form(input): Form<FormInput>,
) -> Response {
    let back = format!("/embed/{}", target.site_key);
    let c = match do_submit(&state, &target, ip, &headers, input).await {
        Ok(c) => c,
        Err(problem) => {
            let reason = problem_text(&problem);
            return Redirect::to(&format!("{back}?error={}", urlencode(&reason))).into_response();
        }
    };
    let path = page_path(target.path.as_deref()).unwrap_or_else(|_| "/".to_owned());
    let mut to = format!("{back}{path}");
    if c.status != "visible" {
        to.push_str("?sent=1");
    }
    Redirect::to(&to).into_response()
}

async fn do_submit(
    state: &AppState,
    target: &Target,
    ip: Option<std::net::IpAddr>,
    headers: &HeaderMap,
    input: FormInput,
) -> Result<Created> {
    let site = site::by_key(state, &target.site_key).await?;
    public::submit(
        state,
        &site,
        ip,
        headers,
        CreateComment {
            path: input.path,
            body_md: input.body_md,
            parent_id: None,
            title: None,
            author_name: input.author_name,
            author_email: input.author_email,
            author_avatar: None,
            author_uid: None,
            challenge_token: None,
            idempotency_key: None,
        },
        true,
    )
    .await
}

fn problem_text(problem: &Problem) -> String {
    match problem {
        Problem::Fields(fields) => fields
            .iter()
            .map(|(_, msg)| msg.as_str())
            .collect::<Vec<_>>()
            .join("；"),
        Problem::Code(code, msg) => format!("{}: {msg}", code.as_str()),
        Problem::Inner(err) => err.to_string(),
    }
}

/// 极简百分号编码，只处理查询串里会出问题的字节。
fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 3);
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// embed 页上「在新页读评论」的地址，widget 降级时用。
pub fn link_for(site_key: &str, path: &str) -> String {
    format!("/embed/{site_key}{}", path.trim_end_matches('/'))
}
