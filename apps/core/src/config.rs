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
    pub turnstile_site_key: Option<String>,
    pub widget_cdn: String,
    pub db_pool_max: u32,
    /// 允许 verify 抓回环/私网地址（本地调试用）。
    pub verify_private: bool,
    pub cors_origins: Vec<String>,
}

fn var(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
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
            widget_cdn,
            db_pool_max,
            verify_private,
            cors_origins,
        })
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
