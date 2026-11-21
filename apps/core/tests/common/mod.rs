//! 冒烟用的最小测试台：真连 PG + Redis，每例一个独立 schema。
//!
//! 依赖由 `IC_TEST_DATABASE_URL` / `IC_TEST_REDIS_URL` 提供，缺一个就整体跳过。

use std::cell::Cell;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use icomment_core::{
    AppState, auth,
    config::{self, Settings},
    domain::account,
    router,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;

pub const V1: &str = "/api";

pub struct Harness {
    pub state: AppState,
    app: Router,
    pub admin: PgPool,
    schema: String,
    /// 伪来源 IP，让每个用例落在自己的限流桶里。
    ip: Cell<String>,
}

/// 便捷断言用：(状态码, JSON)。
pub type Json = (StatusCode, serde_json::Value);

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// 每个用例独立的附图目录，跑完不管。
fn tmp_upload_dir() -> String {
    let dir = std::env::temp_dir().join(format!("ic-up-{}", uuid::Uuid::now_v7().simple()));
    dir.to_string_lossy().into_owned()
}

pub async fn to_json(resp: Response) -> Json {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .expect("body too large");
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

/// 取文本正文（HTML / markdown 页）。
pub async fn text(resp: Response) -> (StatusCode, String) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .expect("body too large");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

impl Harness {
    /// 建独立 schema、跑迁移、装 app；缺依赖返回 None，用例据此跳过。
    pub async fn start() -> Option<Self> {
        Self::start_with(|_| {}).await
    }

    /// 同上，但允许在建好配置、起 app 之前改几个开关（例如指向本地桩）。
    pub async fn start_with(tweak: impl FnOnce(&mut config::Config)) -> Option<Self> {
        let base = env("IC_TEST_DATABASE_URL")?;
        let redis_url = env("IC_TEST_REDIS_URL")?;
        let schema = format!("t{}", uuid::Uuid::now_v7().simple());

        let root = PgPoolOptions::new()
            .max_connections(1)
            .connect(&base)
            .await
            .ok()?;
        sqlx::query(sqlx::AssertSqlSafe(format!("create schema {schema}")))
            .execute(&root)
            .await
            .ok()?;

        // 连接一建立就落在这个 schema 里，迁移、应用、断言三方才不会分歧
        let scoped = format!("{base}?options=-c%20search_path%3D{schema}");
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&scoped)
            .await
            .ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&scoped)
            .await
            .ok()?;
        root.close().await;
        sqlx::migrate!("./migrations").run(&pool).await.ok()?;

        let mut cfg = config::Config {
            bind: "127.0.0.1:0".to_owned(),
            database_url: scoped,
            redis_url: redis_url.clone(),
            public_base_url: "http://127.0.0.1:8080".to_owned(),
            session_secret: "test".to_owned(),
            turnstile_secret: None,
            turnstile_site_key: None,
            // 冒烟默认不连外网；要测 Akismet 的用例自己起桩并设环境变量
            akismet_key: env("IC_TEST_AKISMET_ENDPOINT").map(|_| "test-key".to_owned()),
            akismet_endpoint: env("IC_TEST_AKISMET_ENDPOINT")
                .unwrap_or_else(|| "http://127.0.0.1:9/comment-check".to_owned()),
            widget_cdn: "http://127.0.0.1:8080/widget/v1".to_owned(),
            db_pool_max: 4,
            verify_private: false,
            cors_origins: Vec::new(),
            upload_dir: tmp_upload_dir(),
            // 有 filer 就测真后端，否则落临时目录
            seaweed_filer_url: env("IC_TEST_SEAWEED_URL"),
        };
        tweak(&mut cfg);

        let redis = redis::Client::open(redis_url)
            .ok()?
            .get_connection_manager()
            .await
            .ok()?;
        let state = AppState::new(pool, redis, Settings(cfg), reqwest::Client::new());
        let ip = Cell::new(format!("10.0.0.{}", 1 + (std::process::id() % 250)));

        Some(Self {
            app: router::build(state.clone()).merge(icomment_core::pages::build(state.clone())),
            state,
            admin,
            schema,
            ip,
        })
    }

    /// 换成没人用过的伪 IP：进程号 + 纳秒时钟 + 序号，跨轮次也不撞限流桶。
    pub fn unique_ip(&mut self) {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .subsec_nanos();
        let n = std::process::id()
            .wrapping_mul(7)
            .wrapping_add(nanos)
            .wrapping_add(seq * 1024);
        self.ip
            .set(format!("10.{}.{}.{}", n >> 8 & 0xff, n & 0xff, 1 + n % 200));
    }

    pub async fn finish(self) {
        let sql = sqlx::AssertSqlSafe(format!("drop schema if exists {} cascade", self.schema));
        let _ = sqlx::query(sql).execute(&self.admin).await;
    }

    pub async fn raw(
        &self,
        method: &str,
        uri: &str,
        headers: &[(&str, &str)],
        body: Option<serde_json::Value>,
    ) -> Response {
        // Cell::take 会留下空串，取出来再放回去
        let ip = self.ip.take();
        self.ip.set(ip.clone());
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-forwarded-for", ip);
        if let Some(json) = &body {
            builder = builder.header(header::CONTENT_LENGTH, json.to_string().len());
        }
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder
            .body(body.map_or_else(Body::empty, |json| Body::from(json.to_string())))
            .expect("valid request");
        self.app
            .clone()
            .oneshot(request)
            .await
            .expect("send failed")
    }

    /// 走登录表单拿会话 cookie。
    pub async fn login_web(&self, email: &str) -> String {
        let resp = self
            .raw_form("POST", "/login", &format!("email={email}"))
            .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER, "login");
        let raw = resp
            .headers()
            .get(header::SET_COOKIE)
            .expect("set-cookie")
            .to_str()
            .expect("ascii");
        raw.split(';').next().expect("pair").to_owned()
    }

    /// 带 `x-site-key` 的匿名公开面调用。
    pub async fn anon(&self, uri: &str, key: &str, body: serde_json::Value) -> Json {
        let resp = self
            .raw("POST", uri, &[("x-site-key", key)], Some(body))
            .await;
        to_json(resp).await
    }

    pub async fn get_anon(&self, uri: &str, key: &str) -> Json {
        let resp = self.raw("GET", uri, &[("x-site-key", key)], None).await;
        to_json(resp).await
    }

    /// 带宿主站签名身份发评论。
    pub async fn with_identity(
        &self,
        uri: &str,
        key: &str,
        identity: &str,
        body: serde_json::Value,
    ) -> Json {
        let resp = self
            .raw(
                "POST",
                uri,
                &[("x-site-key", key), ("x-ic-identity", identity)],
                Some(body),
            )
            .await;
        to_json(resp).await
    }

    pub async fn get(&self, uri: &str, bearer: &str) -> Json {
        let resp = self
            .raw(
                "GET",
                uri,
                &[("authorization", &format!("Bearer {bearer}"))],
                None,
            )
            .await;
        to_json(resp).await
    }

    pub async fn post_auth(&self, uri: &str, bearer: &str, body: serde_json::Value) -> Json {
        let auth = format!("Bearer {bearer}");
        let resp = self
            .raw("POST", uri, &[("authorization", &auth)], Some(body))
            .await;
        to_json(resp).await
    }

    pub async fn patch_auth(&self, uri: &str, bearer: &str, body: serde_json::Value) -> Json {
        let auth = format!("Bearer {bearer}");
        let resp = self
            .raw("PATCH", uri, &[("authorization", &auth)], Some(body))
            .await;
        to_json(resp).await
    }

    /// multipart 上传一张图；`bytes` 由调用方给（可以是假魔数）。
    pub async fn post_multipart(&self, uri: &str, key: &str, bytes: &[u8]) -> Response {
        const BOUNDARY: &str = "XBOUNDARY";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\n\
                 Content-Type: image/png\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

        let ip = self.ip.take();
        self.ip.set(ip.clone());
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .header("x-forwarded-for", ip)
            .header("x-site-key", key)
            .body(Body::from(body))
            .expect("valid request");
        self.app
            .clone()
            .oneshot(request)
            .await
            .expect("send failed")
    }

    /// 取一张图（`to_json` 只有 1 MiB，这里直出响应）。
    pub async fn media(&self, file: &str) -> Response {
        let request = Request::builder()
            .uri(format!("/media/{file}"))
            .body(Body::empty())
            .expect("valid request");
        self.app
            .clone()
            .oneshot(request)
            .await
            .expect("send failed")
    }

    /// 原生表单提交（`application/x-www-form-urlencoded`），用来验无 JS 路径。
    pub async fn raw_form(&self, method: &str, uri: &str, form: &str) -> Response {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("x-forwarded-for", self.ip.take().clone())
            .header(header::CONTENT_LENGTH, form.len())
            .body(Body::from(form.to_owned()))
            .expect("valid request");
        self.ip.set(self.ip.take());
        self.app
            .clone()
            .oneshot(request)
            .await
            .expect("send failed")
    }

    /// 走完设备授权（直接调 domain，跳过网页表单），返回账号级 token。
    pub async fn login(&self, email: &str) -> String {
        let account = account::ensure(&self.state, email).await.expect("account");
        let flow = auth::start_device_flow(self.state.db())
            .await
            .expect("device flow");
        auth::approve_device_code(self.state.db(), &flow.user_code, account.id)
            .await
            .expect("approve");
        let (status, issued) = self
            .post_auth(
                &format!("{V1}/agent/token"),
                "",
                serde_json::json!({ "device_code": flow.device_code }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{issued}");
        issued["token"].as_str().expect("token").to_owned()
    }

    /// 建站并按需关掉审核与验证，返回 (site_key, site_id)。
    pub async fn create_site(&self, token: &str, origin: &str) -> (String, uuid::Uuid) {
        self.create_site_with(token, origin, serde_json::json!({}))
            .await
    }

    /// 覆盖站点参数，例如关掉审核。
    pub async fn create_site_with(
        &self,
        token: &str,
        origin: &str,
        extra: serde_json::Value,
    ) -> (String, uuid::Uuid) {
        let mut payload = serde_json::json!({
            "origin": origin,
            "require_approval": false,
            "challenge_mode": "off",
        });
        for (k, v) in extra.as_object().into_iter().flatten() {
            payload[k] = v.clone();
        }
        let (status, body) = self
            .post_auth(&format!("{V1}/agent/sites"), token, payload)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        (
            body["site_key"].as_str().expect("site_key").to_owned(),
            body["id"].as_str().expect("id").parse().expect("uuid"),
        )
    }

    /// 带 `x-site-key` 的反应投票；`voter` 为空时服务端会发 cookie。
    pub async fn react(
        &self,
        uri: &str,
        key: &str,
        voter: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut headers = vec![("x-site-key", key)];
        if let Some(voter) = voter {
            headers.push(("x-ic-voter", voter));
        }
        let resp = self.raw("POST", uri, &headers, None).await;
        let status = resp.status();
        let cookie = resp
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut json = to_json(resp).await.1;
        if let Some(cookie) = cookie {
            json["_cookie"] = serde_json::Value::from(cookie);
        }
        (status, json)
    }

    pub async fn unreact(
        &self,
        uri: &str,
        key: &str,
        voter: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut headers = vec![("x-site-key", key)];
        if let Some(voter) = voter {
            headers.push(("x-ic-voter", voter));
        }
        let resp = self.raw("DELETE", uri, &headers, None).await;
        to_json(resp).await
    }

    /// 改站点设置。
    pub async fn patch_site(
        &self,
        token: &str,
        site_id: uuid::Uuid,
        patch: serde_json::Value,
    ) -> Json {
        self.patch_auth(&format!("{V1}/agent/sites/{site_id}"), token, patch)
            .await
    }
}
