//! 域名归属校验与登录入口的用例。依赖不齐时自动跳过（见 `Harness::start`）。

mod common;

use axum::http::StatusCode;
use icomment_core::{
    claim,
    config::OAuthApp,
    domain::{account, site},
};
use serde_json::json;

use crate::common::{Harness, V1, text};

fn google_app() -> OAuthApp {
    OAuthApp {
        client_id: "test-client".to_owned(),
        client_secret: "test-secret".to_owned(),
    }
}

#[tokio::test]
async fn claim_hands_out_a_key_and_registers_the_domain() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("claim@example.com").await;
    let (status, body) = h
        .post_auth(
            &format!("{V1}/agent/sites/claim"),
            &token,
            json!({ "origin": "claim.example.com" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let key = body["claim_key"].as_str().expect("claim_key");
    assert_eq!(key.len(), 32);
    assert_eq!(
        body["file_url"],
        format!("https://claim.example.com/{key}.txt")
    );
    assert_eq!(body["file_content"], key);
    assert_eq!(body["origin"], "https://claim.example.com");

    // 同一个域名再申请一次，拿回同一条，不重复发 key
    let (status, again) = h
        .post_auth(
            &format!("{V1}/agent/sites/claim"),
            &token,
            json!({ "origin": "claim.example.com" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["claim_key"], key);

    // 测试环境不抓外网（claim_verify=false），直接走完校验
    let owner = account::ensure(&h.state, "claim@example.com")
        .await
        .expect("account");
    let pending = claim::by_key(&h.state, owner.id, "claim.example.com", key)
        .await
        .expect("claim");
    let (_, site) = claim::verify(
        &h.state,
        owner.id,
        pending,
        site::CreateSite {
            origin: String::new(),
            name: Some("Claimed".to_owned()),
            require_approval: Some(false),
            challenge_mode: Some("off".to_owned()),
            blocked_domains: None,
        },
    )
    .await
    .expect("verify");
    assert_eq!(site.origin, "https://claim.example.com");

    // 同一条申请不能再用第二次
    let (_, err) = h
        .post_auth(
            &format!("{V1}/agent/sites/claim"),
            &token,
            json!({ "origin": "claim.example.com" }),
        )
        .await;
    assert_eq!(err["code"], "validation", "{err}");
}

#[tokio::test]
async fn claim_refuses_domains_someone_already_owns() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let owner = h.login("owner@example.com").await;
    h.create_site(&owner, "https://taken.example.com").await;

    let intruder = h.login("intruder@example.com").await;
    let (status, body) = h
        .post_auth(
            &format!("{V1}/agent/sites/claim"),
            &intruder,
            json!({ "origin": "taken.example.com" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "validation", "{body}");
    assert_eq!(body["fields"][0]["field"], "origin");
}

#[tokio::test]
async fn login_page_only_offers_configured_methods() {
    let Some(mut h) = Harness::start_with(|cfg| {
        cfg.google = Some(google_app());
        cfg.email_login = false;
    })
    .await
    else {
        return;
    };
    h.unique_ip();

    let (status, body) = text(h.raw("GET", "/login", &[], None).await).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("/auth/google"), "{body}");
    assert!(!body.contains("/auth/github"), "github is not configured");
    assert!(!body.contains(r#"name="email""#), "email login is off");

    // 关掉之后表单直投也进不去
    let resp = h
        .raw_form("POST", "/login", "email=someone@example.com")
        .await;
    assert_ne!(resp.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn oauth_callback_needs_the_state_cookie() {
    let Some(mut h) = Harness::start_with(|cfg| {
        cfg.google = Some(google_app());
        cfg.email_login = false;
    })
    .await
    else {
        return;
    };
    h.unique_ip();

    let resp = h
        .raw(
            "GET",
            "/auth/google/callback?code=abc&state=forged",
            &[],
            None,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let location = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        location.starts_with("/login?error="),
        "should bounce back to login, got {location}"
    );

    // 未配置的 provider 直接 404，不去拼授权地址
    let resp = h.raw("GET", "/auth/facebook", &[], None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn oauth_start_sets_a_state_cookie_and_redirects() {
    let Some(mut h) = Harness::start_with(|cfg| {
        cfg.github = Some(OAuthApp {
            client_id: "ghid".to_owned(),
            client_secret: "ghsecret".to_owned(),
        });
        cfg.email_login = false;
    })
    .await
    else {
        return;
    };
    h.unique_ip();

    let resp = h.raw("GET", "/auth/github", &[], None).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let location = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        location.starts_with("https://github.com/login/oauth/authorize?"),
        "{location}"
    );
    assert!(location.contains("client_id=ghid"));
    assert!(
        !location.contains("ghsecret"),
        "secret must stay server side"
    );
    let cookie = resp
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(cookie.starts_with("ic_oauth="), "{cookie}");
    assert!(cookie.contains("HttpOnly"));
}

#[tokio::test]
async fn web_claim_failures_come_back_as_a_page_not_json() {
    let Some(mut h) = Harness::start_with(|cfg| {
        cfg.claim_verify = true;
        cfg.verify_private = false;
    })
    .await
    else {
        return;
    };
    h.unique_ip();

    let cookie = h.login_web("webclaim@example.com").await;

    // 建站第一步只发申请，不发 site
    let resp = h
        .raw_form_with_cookie("POST", "/dash/sites", "origin=localhost", &cookie)
        .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let claim_path = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(claim_path.starts_with("/dash/claims/"), "{claim_path}");

    // 内网域名在测试里直接判失败，不发网络请求
    let resp = h
        .raw_form_with_cookie(
            "POST",
            &format!("{claim_path}/verify"),
            "name=Local",
            &cookie,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let location = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        location.starts_with(&format!("{claim_path}?error=")),
        "{location}"
    );

    // 页面把原因直接摊开，不再吐 JSON
    let (status, body) = text(h.raw("GET", &location, &[("cookie", &cookie)], None).await).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("内网域名"), "{body}");
    assert!(body.contains("class=\"error\""), "{body}");
}
