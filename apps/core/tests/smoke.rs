//! 冒烟用例。依赖不齐时自动跳过（见 `Harness::start`）。

mod common;

use axum::http::StatusCode;
use icomment_core::antispam;
use serde_json::json;

use crate::common::{Harness, V1, to_json};

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
    assert_eq!(body["fields"][0]["field"], "url");
    h.finish().await;
}

#[tokio::test]
async fn daily_soft_cap_forces_review_but_keeps_accepting() {
    let Some(h) = Harness::start().await else {
        return;
    };
    let token = h.login("cap@example.com").await;
    let (key, site_id) = h.create_site(&token, "https://cap.example.com").await;

    // 日计数置到软顶之上，模拟当天已写满
    let mut conn = h.state.redis().clone();
    // SET 回的是 "OK"，别按整数收
    let _: String = redis::cmd("SET")
        .arg(format!("ic:daily:{site_id}"))
        .arg(antispam::DAILY_SOFT_CAP + 1)
        .arg("EX")
        .arg(86_400)
        .query_async(&mut conn)
        .await
        .expect("seed counter");

    let (status, body) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/cap", "body_md": "超过软顶的评论", "author_name": "阿丁" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "pending", "{body}");
    assert_eq!(body["created"], true);

    // 第二次仍然接收，说明是降级而不是拒绝
    let (status, body) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/cap", "body_md": "再来一条" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "pending", "{body}");

    h.finish().await;
}

#[tokio::test]
async fn blocked_domain_lands_in_spam() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("blocked@example.com").await;
    let (key, site) = h.create_site(&token, "blocked.example.com").await;
    // 建站时就带上黑名单，子域应当一起管住
    let (status, body) = h
        .patch_site(
            &token,
            site,
            serde_json::json!({ "blocked_domains": ["https://SEO-spam.example/", "seo-spam.example"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["blocked_domains"][0], "seo-spam.example");
    assert_eq!(body["blocked_domains"].as_array().expect("list").len(), 1);

    let (status, body) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/b", "body_md": "看这个 https://cdn.seo-spam.example/a", "author_name": "阿丁" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "spam", "{body}");

    // 清空后不再拦截
    h.patch_site(&token, site, serde_json::json!({ "blocked_domains": [] }))
        .await;
    let (status, body) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/b", "body_md": "又 https://cdn.seo-spam.example/a", "author_name": "阿丁" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "visible", "{body}");

    h.finish().await;
}

#[tokio::test]
async fn every_moderation_step_leaves_a_trace() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();

    let token = h.login("trace@example.com").await;
    let (key, site) = h.create_site(&token, "trace.example.com").await;

    // 开审核，让新评论落在 pending（visible→visible 不准重复 approve）
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
            serde_json::json!({ "path": "/trace", "body_md": "等一等再上线", "author_name": "小秋" }),
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
    let (status, _) = h
        .post_auth(
            &format!("{V1}/agent/sites/{site}/comments/{id}/moderate"),
            &token,
            serde_json::json!({ "action": "hide", "reason": "看不出重点" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // 一条评论的历史：谁做的、做了什么、为什么，都在
    let (status, one) = h
        .get(
            &format!("{V1}/agent/sites/{site}/audit?comment={id}"),
            &token,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{one}");
    let events = one["events"].as_array().expect("events");
    assert_eq!(events.len(), 2, "{one}");
    assert_eq!(events[0]["action"], "visible");
    assert_eq!(events[1]["action"], "hidden");
    assert_eq!(events[1]["reason"], "看不出重点");
    assert_eq!(events[1]["actor_kind"], "agent");

    // 站点视角能看到指向哪条评论、哪个页面
    let (status, all) = h
        .get(&format!("{V1}/agent/sites/{site}/audit"), &token)
        .await;
    assert_eq!(status, StatusCode::OK, "{all}");
    let latest = all["events"]
        .as_array()
        .expect("events")
        .first()
        .expect("one event");
    assert_eq!(latest["comment_id"], id);
    assert_eq!(latest["thread_path"], "/trace");
    assert!(latest["excerpt"].as_str().unwrap_or("").contains("等一等"));

    // 站点主人页面上读得到
    let cookie = h.login_web("trace@example.com").await;
    let page = h
        .raw(
            "GET",
            &format!("/dashboard/sites/{site}/audit"),
            &[("cookie", &cookie)],
            None,
        )
        .await;
    let (_, text) = crate::common::text(page).await;
    assert!(text.contains("审核留痕"), "{text}");
    assert!(text.contains("/trace"), "{text}");

    h.finish().await;
}

/// 起一个假 Akismet：按请求体里 `comment_content` 是否含 `买粉` 回 true。
async fn akismet_stub() -> String {
    use axum::{Router, routing::post};

    async fn check(body: String) -> &'static str {
        if body.contains("buyfollowers") {
            "true"
        } else {
            "false"
        }
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let app = Router::new().route("/comment-check", post(check));
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/comment-check")
}

#[tokio::test]
async fn akismet_marks_spam_and_survives_outage() {
    let endpoint = akismet_stub().await;
    let Some(mut h) = Harness::start_with(|cfg| {
        cfg.akismet_key = Some("test-key".to_owned());
        cfg.akismet_endpoint = endpoint;
    })
    .await
    else {
        return;
    };
    h.unique_ip();

    let token = h.login("akismet@example.com").await;
    let (key, _site) = h.create_site(&token, "akismet.example.com").await;

    // 本地规则没意见，外部判为垃圾
    let (status, body) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/a", "body_md": "buyfollowers 便宜出", "author_name": "阿丁" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "spam", "{body}");

    // 外部说不是垃圾，就正常可见
    let (status, body) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/a", "body_md": "真心好用，推荐一下下", "author_name": "阿丁" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "visible", "{body}");

    h.finish().await;
}

#[tokio::test]
async fn akismet_outage_falls_back_to_local_rules() {
    // 9 号端口一般没人监听：连不上必须照常收下
    let Some(mut h) = Harness::start_with(|cfg| {
        cfg.akismet_key = Some("test-key".to_owned());
        cfg.akismet_endpoint = "http://127.0.0.1:9/comment-check".to_owned();
    })
    .await
    else {
        return;
    };
    h.unique_ip();

    let token = h.login("outage@example.com").await;
    let (key, _site) = h.create_site(&token, "outage.example.com").await;

    let (status, body) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/a", "body_md": "外部服务挂了也别丢评论", "author_name": "阿丁" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "visible", "{body}");

    // 本地已判垃圾的，外部挂了也还是垃圾
    let (status, body) = h
        .anon(
            &format!("{V1}/comments"),
            &key,
            serde_json::json!({ "path": "/a", "body_md": "https://a.example https://b.example https://c.example https://d.example", "author_name": "阿丁" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "spam", "{body}");

    h.finish().await;
}

#[tokio::test]
async fn reactions_count_once_per_voter() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();
    let token = h.login("reactions.example@owner.example").await;
    let (key, _) = h.create_site(&token, "reactions.example").await;
    let created = h
        .anon(
            "/api/v1/comments",
            &key,
            json!({"path": "/r", "body_md": "hi"}),
        )
        .await;
    let comment = &created.1;
    assert_eq!(created.0, StatusCode::OK, "{comment}");
    let id = comment["id"].as_str().expect("id");

    // 同一个投票人连投两次只算一票
    let first = h
        .react(
            &format!("/api/v1/comments/{id}/reactions/up"),
            &key,
            Some("v1"),
        )
        .await;
    assert_eq!(first.0, StatusCode::OK);
    let again = h
        .react(
            &format!("/api/v1/comments/{id}/reactions/up"),
            &key,
            Some("v1"),
        )
        .await;
    assert_eq!(again.1["counts"]["up"], 1);
    assert_eq!(again.1["mine"][0], "up");

    let other = h
        .react(
            &format!("/api/v1/comments/{id}/reactions/heart"),
            &key,
            Some("v2"),
        )
        .await;
    assert_eq!(other.1["counts"]["heart"], 1);
    assert_eq!(other.1["counts"]["up"], 1);

    // 列表里带上票数
    let page = h.get_anon("/api/v1/comments?path=/r", &key).await.1;
    assert_eq!(page["comments"][0]["reactions"]["counts"]["up"], 1);

    // 撤票幂等
    let (_, gone) = h
        .unreact(
            &format!("/api/v1/comments/{id}/reactions/up"),
            &key,
            Some("v1"),
        )
        .await;
    assert!(gone["counts"].get("up").is_none());
    let (_, gone_again) = h
        .unreact(
            &format!("/api/v1/comments/{id}/reactions/up"),
            &key,
            Some("v1"),
        )
        .await;
    assert!(gone_again["mine"].as_array().expect("mine").is_empty());

    h.finish().await;
}

#[tokio::test]
async fn reactions_reject_unknown_kind_and_need_a_voter() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();
    let token = h.login("reaction-bad.example@owner.example").await;
    let (key, _) = h.create_site(&token, "reaction-bad.example").await;
    let created = h
        .anon(
            "/api/v1/comments",
            &key,
            json!({"path": "/r", "body_md": "hi"}),
        )
        .await;
    let comment = &created.1;
    assert_eq!(created.0, StatusCode::OK, "{comment}");
    let id = comment["id"].as_str().expect("id");

    let bad = h
        .react(
            &format!("/api/v1/comments/{id}/reactions/clap"),
            &key,
            Some("v1"),
        )
        .await;
    assert_eq!(bad.0, StatusCode::BAD_REQUEST);
    assert_eq!(bad.1["code"], "validation_failed");

    h.finish().await;
}

#[tokio::test]
async fn anonymous_react_gets_a_signed_voter_cookie() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();
    let token = h.login("reaction-anon.example@owner.example").await;
    let (key, _) = h.create_site(&token, "reaction-anon.example").await;
    let created = h
        .anon(
            "/api/v1/comments",
            &key,
            json!({"path": "/r", "body_md": "hi"}),
        )
        .await;
    let comment = &created.1;
    assert_eq!(created.0, StatusCode::OK, "{comment}");
    let id = comment["id"].as_str().expect("id");

    let (status, body) = h
        .react(
            &format!("/api/v1/comments/{id}/reactions/heart"),
            &key,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = body["_cookie"].as_str().expect("cookie");
    assert!(cookie.starts_with("ic_voter="));
    assert!(!cookie.contains("HttpOnly"));

    h.finish().await;
}

/// 一张 640x480 的 PNG（IHDR 尺寸真实，CRC 不校验），多传几次就是同一份内容。
const PNG: [u8; 67] = [
    0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 2,
    128, 0, 0, 1, 224, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 10, b'I', b'D', b'A', b'T',
    0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0, 0, 0, 0,
    b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
];

async fn upload(h: &Harness, key: &str) -> (StatusCode, serde_json::Value) {
    to_json(h.post_multipart(&format!("{V1}/uploads"), key, &PNG).await).await
}

async fn post_with_images(
    h: &Harness,
    key: &str,
    snippet: &str,
) -> (StatusCode, serde_json::Value) {
    h.anon(
        &format!("{V1}/comments"),
        key,
        serde_json::json!({
            "path": "/posts/pics",
            "body_md": format!("看图 {snippet}"),
        }),
    )
    .await
}

#[tokio::test]
async fn upload_then_comment_binds_the_image() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();
    let token = h.login("pic@example.com").await;
    let (key, _) = h.create_site(&token, "pic.example.com").await;

    let (status, up) = upload(&h, &key).await;
    assert_eq!(status, StatusCode::OK, "{up}");
    assert_eq!(up["width"], 640);
    assert_eq!(up["height"], 480);
    let markdown = up["markdown"].as_str().expect("markdown");
    assert!(markdown.contains("ic-attachment:"));

    let (status, posted) = post_with_images(&h, &key, markdown).await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    let html = posted["body_html"].as_str().expect("body_html");
    assert!(html.contains("<img"), "{html}");
    assert!(!html.contains("ic-attachment:"), "{html}");
    let path = up["url"]
        .as_str()
        .expect("url")
        .rsplit_once("/media/")
        .expect("absolute url")
        .1
        .to_owned();
    // CAS：文件名就是内容的 sha256，两次上传同一张图得到同一个地址
    assert_eq!(path, icomment_core::blob::key_of(&PNG));
    assert!(html.contains(&path), "{html}");
    assert_eq!(posted["attachments"].as_array().expect("list").len(), 1);

    let resp = h.media(&path).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").expect("type"),
        "image/png"
    );
    let etag = resp
        .headers()
        .get("etag")
        .expect("etag")
        .to_str()
        .expect("ascii")
        .to_owned();
    let cached = h
        .raw(
            "GET",
            &format!("/media/{path}"),
            &[("if-none-match", etag.as_str())],
            None,
        )
        .await;
    assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
    assert!(h.media("nope.png").await.status() == StatusCode::NOT_FOUND);

    h.finish().await;
}

#[tokio::test]
async fn attachment_rules_are_enforced() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();
    let token = h.login("rule@example.com").await;
    let (key, _) = h.create_site(&token, "rule.example.com").await;
    let (other, _) = h.create_site(&token, "other.example.com").await;

    let (status, bad) = to_json(
        h.post_multipart(&format!("{V1}/uploads"), &key, b"<svg/>")
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");

    // 别的站的附件不能拿来贴
    let (_, foreign) = upload(&h, &other).await;
    let markdown = foreign["markdown"].as_str().expect("markdown").to_owned();
    let (status, posted) = post_with_images(&h, &key, &markdown).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{posted}");

    // 一张图只能绑一次
    let (_, up) = upload(&h, &key).await;
    let markdown = up["markdown"].as_str().expect("markdown").to_owned();
    assert_eq!(
        post_with_images(&h, &key, &markdown).await.0,
        StatusCode::OK
    );
    h.unique_ip();
    assert_eq!(
        post_with_images(&h, &key, &markdown).await.0,
        StatusCode::BAD_REQUEST
    );

    // 第五张只保留前四张
    let mut snippets = Vec::new();
    for _ in 0..5 {
        let (_, up) = upload(&h, &key).await;
        snippets.push(up["markdown"].as_str().expect("markdown").to_owned());
    }
    let (status, posted) = post_with_images(&h, &key, &snippets.join("\n")).await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert_eq!(posted["attachments"].as_array().expect("list").len(), 4);

    h.finish().await;
}

#[tokio::test]
async fn identical_bytes_are_stored_once() {
    let Some(mut h) = Harness::start().await else {
        return;
    };
    h.unique_ip();
    let token = h.login("cas@example.com").await;
    let (key, _) = h.create_site(&token, "cas.example.com").await;

    let (_, first) = upload(&h, &key).await;
    h.unique_ip();
    let (_, second) = upload(&h, &key).await;
    assert_eq!(first["url"], second["url"]);
    assert_ne!(first["id"], second["id"]);

    // 同一张图两条评论都贴，引用两处而字节只有一份
    let snippet = format!("{} {}", first["markdown"], second["markdown"]);
    let (status, posted) = post_with_images(&h, &key, &snippet).await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert_eq!(posted["attachments"].as_array().expect("list").len(), 2);

    // 两条都没人引用、且放够一天时才真删字节
    sqlx::query("update blob set created_at = now() - interval '2 days'")
        .execute(&h.admin)
        .await
        .expect("age blob");
    let removed = icomment_core::blob::sweep_orphans(&h.state)
        .await
        .expect("sweep");
    assert_eq!(removed, 0, "还被引用着");
    assert_eq!(h.media("").await.status(), StatusCode::NOT_FOUND);

    // 解绑到没人引用，放够一天就被真删（字节也删）
    sqlx::query("delete from attachment")
        .execute(&h.admin)
        .await
        .expect("unlink");
    let hash = first["url"]
        .as_str()
        .expect("url")
        .rsplit('/')
        .next()
        .expect("hash")
        .to_owned();
    assert!(icomment_core::blob::get(&h.state, &hash).await.is_some());
    assert_eq!(
        icomment_core::blob::sweep_orphans(&h.state)
            .await
            .expect("sweep"),
        1
    );
    assert!(icomment_core::blob::get(&h.state, &hash).await.is_none());

    h.finish().await;
}
