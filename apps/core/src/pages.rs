//! 网页面路由。

use axum::{
    Router,
    routing::{get, post},
};

use crate::{discover, embed, state::AppState, web};

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/", get(web::login_page))
        // agent 的发现入口
        .route("/llms.txt", get(discover::llms))
        .route("/install.md", get(discover::install))
        .route("/login", get(web::login_page).post(web::login_submit))
        .route("/logout", get(web::logout))
        .route("/dashboard", get(web::dashboard))
        .route("/dashboard/sites", post(web::create_site))
        .route("/dashboard/sites/{site_id}", get(web::site_page))
        .route(
            "/dashboard/sites/{site_id}/settings",
            post(web::save_settings),
        )
        .route(
            "/dashboard/sites/{site_id}/threads/{thread_id}",
            get(web::thread_page),
        )
        .route("/dashboard/sites/{site_id}/audit", get(web::audit_page))
        .route(
            "/dashboard/comments/{comment_id}/moderate",
            post(web::moderate),
        )
        .route("/device", get(web::device_page).post(web::device_submit))
        .route("/tokens", get(web::token_page))
        .route("/tokens/{token_id}/revoke", post(web::revoke_token))
        // 无 JS 兜底：widget 加载失败时访客仍能读能写
        .route("/embed/{site_key}", get(embed::show).post(embed::submit))
        .route(
            "/embed/{site_key}/{*path}",
            get(embed::show).post(embed::submit),
        )
        .with_state(state)
}
