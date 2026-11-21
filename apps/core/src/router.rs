//! 路由装配。公开面与 agent 面靠前缀分开，中间件按层挂。

use axum::middleware::Next;
use axum::{
    Router,
    body::Body,
    http::{StatusCode, header},
    response::IntoResponse,
    response::Response,
    routing::{get, post},
};
use tower_http::{
    catch_panic::CatchPanicLayer, compression::CompressionLayer, cors::CorsLayer,
    limit::RequestBodyLimitLayer, timeout::TimeoutLayer, trace::TraceLayer,
};

use crate::{api, domain::comment::MAX_BODY_BYTES, state::AppState, upload};

/// 全局体积上限按附图算；上传失败时由 handler 自己再收窄到 4 MiB。
pub const BODY_LIMIT: usize = upload::MAX_FILE_BYTES + 64 * 1024;

pub fn build(state: AppState) -> Router {
    let public = Router::new()
        .route("/config", get(api::public::config))
        .route("/site", get(api::public::site_meta))
        .route(
            "/comments",
            get(api::public::list).post(api::public::create),
        )
        .route("/comments/{id}", get(api::public::one))
        .route("/uploads", post(api::upload::create))
        .route(
            "/comments/{id}/reactions/{kind}",
            post(api::public::react).delete(api::public::unreact),
        );

    let agent = Router::new()
        .route("/me", get(api::agent::me))
        .route("/tokens/revoke", post(api::agent::revoke_token))
        .route(
            "/sites",
            get(api::agent::list_sites).post(api::agent::create_site),
        )
        .route(
            "/sites/{site_id}",
            get(api::agent::get_site)
                .patch(api::agent::patch_site)
                .delete(api::agent::delete_site),
        )
        .route(
            "/sites/{site_id}/comments",
            get(api::agent::list_comments).post(api::agent::create_comment),
        )
        .route(
            "/sites/{site_id}/comments/{comment_id}/moderate",
            post(api::agent::moderate),
        )
        .route("/sites/{site_id}/threads", get(api::agent::list_threads))
        .route("/sites/{site_id}/audit", get(api::agent::audit_log))
        .route("/sites/{site_id}/stats", get(api::agent::stats))
        .route("/sites/{site_id}/verify", post(api::verify::verify_install))
        .route("/sites/{site_id}/identity", post(api::agent::sign_identity));

    // device flow 在拿到 token 之前就要能用
    let anon = Router::new()
        .route("/agent/device", post(api::agent::device_start))
        .route("/agent/token", post(api::agent::device_token));

    Router::new()
        .route("/health", get(health))
        .route("/widget/v1/icomment.js", get(widget_js))
        .route("/static/app.css", get(app_css))
        .route("/media/{file}", get(api::upload::media))
        .nest("/api/v1", public.merge(anon))
        .nest("/api/v1/agent", agent)
        .fallback(not_found)
        .layer(axum::middleware::from_fn(version_header))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            std::time::Duration::from_secs(15),
        ))
        .layer(CompressionLayer::new())
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(BODY_LIMIT.max(MAX_BODY_BYTES + 8 * 1024)))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any)
                .max_age(std::time::Duration::from_secs(600)),
        )
        .with_state(state)
}

/// 每个响应都带版本号，方便站点侧排查缓存与兼容问题。
async fn version_header(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::HeaderName::from_static("x-ic-version"),
        header::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
    );
    response
}

/// 内嵌的 widget 构建产物：单二进制自托管，不需要外部 CDN。
async fn widget_js() -> Response {
    const SCRIPT: &[u8] = include_bytes!("../assets/icomment.js");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
        .header(header::CACHE_CONTROL, "public, max-age=60, must-revalidate")
        .body(Body::from(SCRIPT))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn app_css() -> Response {
    const STYLE: &[u8] = include_bytes!("../assets/app.css");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .header(
            header::CACHE_CONTROL,
            "public, max-age=300, must-revalidate",
        )
        .body(Body::from(STYLE))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// 兜底：API 面回 problem+json，其余交给网页侧的 404 页面。
async fn not_found(uri: axum::http::Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return crate::error::Problem::code(
            crate::error::Code::Validation,
            format!("no route for {}", uri.path()),
        )
        .into_response();
    }
    crate::web::not_found_web().await.into_response()
}
