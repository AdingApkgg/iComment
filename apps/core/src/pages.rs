//! 网页面路由。

use axum::{
    Router,
    routing::{get, post},
};

use crate::{discover, embed, state::AppState, web};

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/", get(web::landing))
        // agent 的发现入口
        .route("/llms.txt", get(discover::llms))
        .route("/install.md", get(discover::install))
        .route("/login", get(web::login_page).post(web::login_submit))
        .route("/logout", get(web::logout))
        .route("/auth/{provider}", get(web::oauth_start))
        .route("/auth/{provider}/callback", get(web::oauth_callback))
        .route("/dash", get(web::dashboard))
        .route("/dash/sites", post(web::create_site))
        .route("/dash/claims/{claim_id}", get(web::claim_page))
        .route("/dash/claims/{claim_id}/verify", post(web::claim_verify))
        .route("/dash/sites/{site_id}", get(web::site_page))
        .route("/dash/sites/{site_id}/settings", post(web::save_settings))
        .route(
            "/dash/sites/{site_id}/threads/{thread_id}",
            get(web::thread_page),
        )
        .route("/dash/sites/{site_id}/audit", get(web::audit_page))
        .route("/dash/comments/{comment_id}/moderate", post(web::moderate))
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
