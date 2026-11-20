//! 冒烟用例。依赖不齐时自动跳过（见 `Harness::start`）。

mod common;

use axum::http::StatusCode;

use crate::common::{Harness, V1};

#[tokio::test]
async fn device_flow_issues_working_token() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("flow@example.com").await;
    let (status, me) = h.get(&format!("{V1}/agent/me"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["email"], "flow@example.com");

    // 撤掉自己之后立即失效
    let (status, _) = h
        .post_auth(
            &format!("{V1}/agent/tokens/revoke"),
            &token,
            serde_json::json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = h.get(&format!("{V1}/agent/me"), &token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    h.finish().await;
}

#[tokio::test]
async fn site_token_is_scoped_to_one_site() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let account_token = h.login("scope@example.com").await;
    let (_, site) = h.create_site(&account_token, "own.example.com").await;
    let (_, other) = h.create_site(&account_token, "other.example.com").await;

    let (status, body) = h
        .get(&format!("{V1}/agent/sites/{other}"), &account_token)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 站点级 token 只认自己那个站
    let account_id = {
        let (_, me) = h.get(&format!("{V1}/agent/me"), &account_token).await;
        me["account_id"].as_str().unwrap().parse().unwrap()
    };
    let site_token =
        icomment_core::auth::issue_site_token(h.state.db(), account_id, site, "widget")
            .await
            .expect("site token");

    let (status, _) = h
        .get(&format!("{V1}/agent/sites/{site}"), &site_token)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = h
        .get(&format!("{V1}/agent/sites/{other}"), &site_token)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    h.finish().await;
}

#[tokio::test]
async fn public_post_renders_markdown_and_paginates() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("post@example.com").await;
    let (key, _) = h.create_site(&token, "post.example.com").await;

    let (status, created) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({
                "path": "/posts/Hello?utm=x#top",
                "body_md": "看这里 **粗体** 和 [链接](https://example.com)",
                "author_name": "小夏",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let body = created["body_html"].as_str().expect("body_html");
    assert!(body.contains("<strong>粗体</strong>"), "{body}");
    assert!(
        body.contains(r#"rel="noopener noreferrer nofollow""#),
        "{body}"
    );

    // 大写 path 归一化后仍是同一个线程
    let (status, meta) = h
        .get_anon(&format!("{V1}/site?path=/POSTS/hello"), &key)
        .await;
    assert_eq!(status, StatusCode::OK, "{meta}");

    // GET 与 POST 共用 /comments
    let resp = h
        .raw(
            "GET",
            &format!("{V1}/comments?path=/posts/hello"),
            &[("x-site-key", key.as_str())],
            None,
        )
        .await;
    let (status, page) = crate::common::to_json(resp).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["comments"].as_array().expect("list").len(), 1);

    h.finish().await;
}

#[tokio::test]
async fn burst_limit_kicks_in_on_fourth_comment() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("burst@example.com").await;
    let (key, _) = h.create_site(&token, "burst.example.com").await;

    for i in 0..3 {
        let (status, body) = h
            .anon(
                &format!("{V1}/comments"),
                &key,
                serde_json::json!({ "path": "/p", "body_md": format!("第 {i} 条"), "author_name": "小夏" }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let resp = h
        .raw(
            "POST",
            &format!("{V1}/comments"),
            &[("x-site-key", key.as_str())],
            Some(serde_json::json!({ "path": "/p", "body_md": "第四条" })),
        )
        .await;
    let status = resp.status();
    let has_retry = resp.headers().contains_key("retry-after");
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(has_retry);

    h.finish().await;
}

#[tokio::test]
async fn approval_gate_and_moderation() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("mod@example.com").await;
    let (key, site) = h.create_site(&token, "mod.example.com").await;

    // 开审核
    let (status, _) = h
        .patch_auth(
            &format!("{V1}/agent/sites/{site}"),
            &token,
            serde_json::json!({ "require_approval": true }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, created) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/p", "body_md": "待审的一条呀", "author_name": "小夏" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["status"], "pending", "{created}");
    let id = created["id"].as_str().expect("id");

    let (status, _) = h
        .post_auth(
            &format!("{V1}/agent/sites/{site}/comments/{id}/moderate"),
            &token,
            serde_json::json!({ "action": "approve" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (_, page) = h.get_anon(&format!("{V1}/comments?path=/p"), &key).await;
    assert_eq!(page["comments"][0]["status"], "visible");

    // 删除是终态：再 approve 会被拒
    let (status, body) = h
        .post_auth(
            &format!("{V1}/agent/sites/{site}/comments/{id}/moderate"),
            &token,
            serde_json::json!({ "action": "delete" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = h
        .post_auth(
            &format!("{V1}/agent/sites/{site}/comments/{id}/moderate"),
            &token,
            serde_json::json!({ "action": "approve" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "validation_failed");

    h.finish().await;
}

#[tokio::test]
async fn site_key_beats_forged_origin() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("origin@example.com").await;
    let (key, _) = h.create_site(&token, "real.example.com").await;

    let resp = h
        .raw(
            "GET",
            &format!("{V1}/site?path=/p"),
            &[
                ("x-site-key", key.as_str()),
                ("origin", "https://forged.example.com"),
            ],
            None,
        )
        .await;
    let (status, body) = crate::common::to_json(resp).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["origin"], "https://real.example.com");

    h.finish().await;
}
