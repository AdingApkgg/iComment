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

#[tokio::test]
async fn embed_page_works_without_javascript() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("embed@example.com").await;
    let (key, _) = h.create_site(&token, "embed.example.com").await;

    // 表单所在页可读
    let (status, html) = crate::common::text(
        h.raw("GET", &format!("/embed/{key}/posts/hello"), &[], None)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<form method=\"post\""), "{html}");
    assert!(html.contains("不需要 JavaScript"));

    // 原生表单提交：urlencoded，无 Turnstile 令牌
    let form = "path=%2Fposts%2Fhello&body_md=**无JS**&author_name=手敲";
    let resp = h
        .raw_form("POST", &format!("/embed/{key}/posts/hello"), form)
        .await;
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "form post should redirect"
    );

    let (status, html) = crate::common::text(
        h.raw("GET", &format!("/embed/{key}/posts/hello"), &[], None)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<strong>无JS</strong>"), "{html}");
    assert!(html.contains("手敲"));

    // 空正文回跳到带原因的地址
    let resp = h
        .raw_form(
            "POST",
            &format!("/embed/{key}/posts/hello"),
            "path=/posts/hello&body_md=",
        )
        .await;
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(location.contains("error="), "{location}");

    h.finish().await;
}

#[tokio::test]
async fn idempotency_header_prevents_double_post() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("idem@example.com").await;
    let (key, _) = h.create_site(&token, "idem.example.com").await;
    let body = serde_json::json!({ "path": "/p", "body_md": "只该出现一次" });

    let first = h
        .raw(
            "POST",
            &format!("{V1}/comments"),
            &[("x-site-key", key.as_str()), ("idempotency-key", "k-1")],
            Some(body.clone()),
        )
        .await;
    let (status, created) = crate::common::to_json(first).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["created"], true);

    let again = h
        .raw(
            "POST",
            &format!("{V1}/comments"),
            &[("x-site-key", key.as_str()), ("idempotency-key", "k-1")],
            Some(body),
        )
        .await;
    let (status, duplicate) = crate::common::to_json(again).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(duplicate["created"], false);
    assert_eq!(duplicate["id"], created["id"]);

    h.finish().await;
}

#[tokio::test]
async fn discovery_documents_are_served() {
    let Some(h) = Harness::start().await else {
        return;
    };

    let (status, body) = crate::common::text(h.raw("GET", "/llms.txt", &[], None).await).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("/api/v1/agent/device"), "{body}");

    let (status, body) = crate::common::text(h.raw("GET", "/install.md", &[], None).await).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("data-site-key"));

    h.finish().await;
}

#[tokio::test]
async fn signed_identity_skips_review_and_challenge() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("ident@example.com").await;
    let (key, site) = h.create_site(&token, "ident.example.com").await;
    let path = format!("{V1}/agent/sites/{site}/identity");

    let (status, signed) = h
        .post_auth(
            &path,
            &token,
            serde_json::json!({ "uid": "u-1", "name": "阿丁" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{signed}");
    let header = signed["header"].as_str().expect("header").to_owned();
    assert_eq!(signed["expires_in"], 300);

    // 验过的身份：免待审、作者名取签名里的
    let (status, created) = h
        .with_identity(
            &format!("{V1}/comments"),
            &key,
            &header,
            serde_json::json!({ "path": "/signed", "body_md": "站点自己的用户发的" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["status"], "visible", "{created}");
    let id = created["id"].as_str().expect("id").to_owned();

    let (status, page) = h
        .get_anon(&format!("{V1}/comments?path=%2Fsigned"), &key)
        .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let mine = page["comments"]
        .as_array()
        .expect("array")
        .iter()
        .find(|c| c["id"] == id.as_str())
        .expect("comment present")
        .clone();
    assert_eq!(mine["author_name"], "阿丁", "{mine}");

    // 改签名中间一个字符就降级：仍入库，但标成未验证并按匿名处理。
    // 注意别改最后一个字符——base64 尾部有填充位，翻它可能仍是同一串字节。
    let mut tampered: Vec<char> = header.chars().collect();
    let mid = tampered.len() - 8;
    tampered[mid] = if tampered[mid] == 'A' { 'B' } else { 'A' };
    let tampered: String = tampered.into_iter().collect();
    let (status, created) = h
        .with_identity(
            &format!("{V1}/comments"),
            &key,
            &tampered,
            serde_json::json!({ "path": "/forged", "body_md": "签名被改过的一条" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");

    let raw: i64 = sqlx::query_scalar(
        "select count(*) from comment where body_md = '签名被改过的一条' and identity_unverified",
    )
    .fetch_one(&h.admin)
    .await
    .expect("count");
    assert_eq!(raw, 1);

    h.finish().await;
}

#[tokio::test]
async fn embed_snippet_does_not_double_the_file_name() {
    let Some(h) = common::Harness::start().await else {
        return;
    };
    let token = h.login("snippet@example.com").await;
    let (key, _site_id) = h.create_site(&token, "https://snippet.example.com").await;
    for cdn in [
        "http://127.0.0.1:8080/widget/v1",
        "http://cdn.example.com/icomment.js",
    ] {
        let mut cfg = h.state.cfg().clone();
        cfg.widget_cdn = cdn.to_owned();
        let snippet = icomment_core::api::agent::embed_snippet(&h.state.with_cfg(cfg), &key);
        assert_eq!(snippet.matches("icomment.js").count(), 1, "{snippet}");
        assert!(
            snippet.contains(&format!("data-site-key=\"{key}\"")),
            "{snippet}"
        );
    }
    h.finish().await;
}

#[tokio::test]
async fn verify_reports_a_private_host_with_an_actionable_hint() {
    let Some(h) = common::Harness::start().await else {
        return;
    };
    let token = h.login("verify-local@example.com").await;
    let (_key, site_id) = h
        .create_site(&token, "https://verify-local.example.com")
        .await;
    let (status, body) = h
        .post_auth(
            &format!("/api/v1/agent/sites/{site_id}/verify"),
            &token,
            serde_json::json!({ "url": "http://127.0.0.1:8090/post" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "failed");
    assert_eq!(body["reason"], "private_host");
    assert!(
        body["hint"]
            .as_str()
            .is_some_and(|h| h.contains("IC_VERIFY_PRIVATE"))
    );
    assert!(body["http_status"].is_null());
    h.finish().await;
}

#[tokio::test]
async fn verify_rejects_a_relative_url() {
    let Some(h) = common::Harness::start().await else {
        return;
    };
    let token = h.login("verify-url@example.com").await;
    let (_key, site_id) = h
        .create_site(&token, "https://verify-url.example.com")
        .await;
    let (status, body) = h
        .post_auth(
            &format!("/api/v1/agent/sites/{site_id}/verify"),
            &token,
            serde_json::json!({ "url": "ftp://example.com/x" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "validation_failed");
    assert!(body["fields"]["url"].is_string());
    h.finish().await;
}
