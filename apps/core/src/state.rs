//! 共享状态。连接池与 Redis 连接在启动时建一次，handler 只取引用。

use std::sync::Arc;

use axum::extract::FromRef;
use redis::aio::ConnectionManager;
use sqlx::PgPool;

use crate::config::Settings;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub db: PgPool,
    pub redis: ConnectionManager,
    pub cfg: Settings,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(db: PgPool, redis: ConnectionManager, cfg: Settings, http: reqwest::Client) -> Self {
        Self(Arc::new(Inner {
            db,
            redis,
            cfg,
            http,
        }))
    }

    pub fn db(&self) -> &PgPool {
        &self.0.db
    }

    pub fn redis(&self) -> &ConnectionManager {
        &self.0.redis
    }

    pub fn cfg(&self) -> &Settings {
        &self.0.cfg
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.0.http
    }

    /// 换一份配置（测试里改单个开关用）。
    pub fn with_cfg(&self, cfg: Settings) -> Self {
        Self(Arc::new(Inner {
            db: self.0.db.clone(),
            redis: self.0.redis.clone(),
            cfg,
            http: self.0.http.clone(),
        }))
    }
}

impl FromRef<AppState> for Settings {
    fn from_ref(state: &AppState) -> Self {
        state.0.cfg.clone()
    }
}

impl FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.0.db.clone()
    }
}
