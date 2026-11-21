//! 配置：全部来自环境变量，启动即校验，缺关键项直接退出（不带着半配跑）。

use std::env;

use anyhow::{Context, bail};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: String,
    pub database_url: String,
    pub redis_url: String,
    pub public_base_url: String,
    /// 签会话 cookie 的密钥；dev 下按 base url 派生，prod 必须显式给。
    pub session_secret: String,
    pub turnstile_secret: Option<String>,
    /// Akismet key；配了才会对外发判定请求。
    pub akismet_key: Option<String>,
    /// 附图目录；空则落到进程内存（冒烟用）。
    pub upload_dir: String,
    /// SeaweedFS filer 基址（如 `http://seaweedfs:8888/uploads`）；配了就用它存字节。
    pub seaweed_filer_url: Option<String>,
    /// 判定接口，默认官方地址；自建代理或测试时改这里。
    pub akismet_endpoint: String,
    pub turnstile_site_key: Option<String>,
    pub widget_cdn: String,
    pub db_pool_max: u32,
    /// 允许 verify 抓回环/私网地址（本地调试用）。
    pub verify_private: bool,
    pub cors_origins: Vec<String>,
    /// OAuth 应用；两个都给齐才算这个登录方式可用。
    pub google: Option<OAuthApp>,
    pub github: Option<OAuthApp>,
    /// 保留的邮箱直登，只在不叫 prod 的 profile 下默认开。
    pub email_login: bool,
    /// 建站时的域名归属校验；prod 下不允许关。
    pub claim_verify: bool,
}

/// 一个 OAuth 应用的凭据。
#[derive(Clone)]
pub struct OAuthApp {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for OAuthApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 别让 secret 跟着 Debug 进日志
        f.debug_struct("OAuthApp")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

fn var(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn is_on(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// 只给了一半的 OAuth 配置按错处理，别让人以为配好了却登不进。
fn oauth_app(id_key: &str, secret_key: &str) -> anyhow::Result<Option<OAuthApp>> {
    match (var(id_key), var(secret_key)) {
        (None, None) => Ok(None),
        (Some(client_id), Some(client_secret)) => Ok(Some(OAuthApp {
            client_id,
            client_secret,
        })),
        _ => bail!("{id_key} and {secret_key} must be set together"),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let profile = var("IC_PROFILE").unwrap_or_else(|| "dev".to_owned());
        if profile == "prod" {
            for key in [
                "IC_DATABASE_URL",
                "IC_REDIS_URL",
                "IC_PUBLIC_BASE_URL",
                "IC_SESSION_SECRET",
            ] {
                if var(key).is_none() {
                    bail!("{key} is required in prod profile");
                }
            }
        }

        let database_url = var("IC_DATABASE_URL")
            .or_else(|| var("DATABASE_URL"))
            .unwrap_or_else(|| "postgres://icomment:icomment@127.0.0.1:5432/icomment".to_owned());
        let redis_url = var("IC_REDIS_URL").unwrap_or_else(|| "redis://127.0.0.1:6379".to_owned());
        let public_base_url = var("IC_PUBLIC_BASE_URL")
            .unwrap_or_else(|| "http://127.0.0.1:8080".to_owned())
            .trim_end_matches('/')
            .to_owned();
        let session_secret =
            var("IC_SESSION_SECRET").unwrap_or_else(|| format!("dev-{public_base_url}"));
        let bind = var("IC_BIND").unwrap_or_else(|| "127.0.0.1:8080".to_owned());
        let widget_cdn = var("IC_WIDGET_CDN")
            .unwrap_or_else(|| format!("{public_base_url}/widget/v1"))
            .trim_end_matches('/')
            .to_owned();

        let cors_origins = var("IC_CORS_ORIGINS")
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let google = oauth_app("IC_GOOGLE_CLIENT_ID", "IC_GOOGLE_CLIENT_SECRET")?;
        let github = oauth_app("IC_GITHUB_CLIENT_ID", "IC_GITHUB_CLIENT_SECRET")?;
        let email_login = match var("IC_EMAIL_LOGIN") {
            Some(v) => is_on(&v),
            None => profile != "prod",
        };
        let claim_verify = match var("IC_CLAIM_VERIFY") {
            Some(v) => is_on(&v),
            None => true,
        };
        if profile == "prod" && !claim_verify {
            bail!("IC_CLAIM_VERIFY=0 is not allowed in prod profile");
        }

        let verify_private = var("IC_VERIFY_PRIVATE").is_some_and(|v| v == "1" || v == "true");
        let db_pool_max = match var("IC_DB_POOL_MAX") {
            Some(raw) => raw.parse().context("IC_DB_POOL_MAX must be a number")?,
            None => 16,
        };

        Ok(Self {
            bind,
            database_url,
            redis_url,
            public_base_url,
            session_secret,
            turnstile_site_key: var("IC_TURNSTILE_SITE_KEY"),
            turnstile_secret: var("IC_TURNSTILE_SECRET"),
            akismet_key: var("IC_AKISMET_KEY"),
            upload_dir: var("IC_UPLOAD_DIR").unwrap_or_default(),
            seaweed_filer_url: var("IC_SEAWEED_FILER_URL"),
            akismet_endpoint: var("IC_AKISMET_ENDPOINT")
                .unwrap_or_else(|| "https://rest.akismet.com/1.1/comment-check".to_owned()),
            widget_cdn,
            db_pool_max,
            verify_private,
            cors_origins,
            google,
            github,
            email_login,
            claim_verify,
        })
    }

    /// 有 filer 或本地目录才谈得上存图。
    pub fn uploads_enabled(&self) -> bool {
        self.seaweed_filer_url.is_some() || !self.upload_dir.trim().is_empty()
    }

    /// 至少配了一个 OAuth 应用，登录页才有按钮可点。
    pub fn oauth_enabled(&self) -> bool {
        self.google.is_some() || self.github.is_some()
    }

    /// 只有两个 key 都配了才算能开挑战，缺一个就整体退化成 off。
    pub fn turnstile_required(&self) -> bool {
        self.turnstile_secret.is_some() && self.turnstile_site_key.is_some()
    }
}

/// 静态配置，供 handler 通过 State 取用。
#[derive(Debug, Clone)]
pub struct Settings(pub Config);

impl std::ops::Deref for Settings {
    type Target = Config;
    fn deref(&self) -> &Config {
        &self.0
    }
}

impl std::ops::DerefMut for Settings {
    fn deref_mut(&mut self) -> &mut Config {
        &mut self.0
    }
}
