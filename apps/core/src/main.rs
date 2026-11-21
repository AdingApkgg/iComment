//! 进程入口：读配置 → 连 PG/Redis → 跑迁移 → 起服务。
//!
//! 额外子命令 `health`：容器健康检查用（distroless 里没有 curl）。

use std::time::Duration;

use anyhow::Context;
use icomment_core::{AppState, config::Settings, router};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("health") {
        return health().await;
    }

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn".into()))
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let cfg = Settings(icomment_core::config::Config::from_env()?);
    tracing::info!(bind = %cfg.bind, "starting icomment-core");

    let db = PgPoolOptions::new()
        .max_connections(cfg.db_pool_max)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&cfg.database_url)
        .await
        .context("connect postgres")?;

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .context("run migrations")?;

    let redis_client = redis::Client::open(cfg.redis_url.clone()).context("redis url")?;
    let redis = redis::aio::ConnectionManager::new(redis_client)
        .await
        .context("connect redis")?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("icomment-core/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build http client")?;

    let state = AppState::new(db, redis, cfg.clone(), http);
    spawn_upload_sweeper(state.clone());
    let app = router::build(state.clone()).merge(icomment_core::pages::build(state));

    let listener = TcpListener::bind(&cfg.bind)
        .await
        .with_context(|| format!("bind {}", cfg.bind))?;
    tracing::info!("listening on {}", listener.local_addr()?);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    Ok(())
}

/// 只探活，不连库；连不上就当失败（编排层据此重启）。
async fn health() -> anyhow::Result<()> {
    let bind = std::env::var("IC_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let host = match bind.rsplit_once(':') {
        Some(("0.0.0.0", _)) | Some(("::", _)) | None => "127.0.0.1".to_owned(),
        Some((host, _)) => host.to_owned(),
    };
    let port = bind.rsplit_once(':').map_or("8080", |(_, p)| p);
    let url = format!("http://{host}:{port}/health");

    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await?;
    anyhow::ensure!(
        resp.status().is_success(),
        "health returned {}",
        resp.status()
    );
    Ok(())
}

/// 每小时收拾一次没被评论引用、已过期的上传。
fn spawn_upload_sweeper(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match icomment_core::domain::attachment::sweep(&state).await {
                Ok(n) if n > 0 => tracing::info!(removed = n, "swept stale uploads"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "upload sweep failed"),
            }
        }
    });
}

async fn shutdown() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    tracing::info!("shutting down");
}
