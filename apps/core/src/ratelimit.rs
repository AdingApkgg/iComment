//! 按站点/账号维度计量，配额写在代码里，先不设闸。

use std::time::Duration;

use anyhow::Context;
use redis::AsyncCommands;

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

/// 某个动作在窗口内的上限。
#[derive(Debug, Clone, Copy)]
pub struct Quota {
    pub bucket: &'static str,
    pub limit: i64,
    pub window: Duration,
}

/// 建站前的域名归属校验：同一个域名每小时最多试 10 次。
pub const CLAIM_PER_ORIGIN: Quota = Quota {
    bucket: "claim",
    limit: 10,
    window: Duration::from_secs(3600),
};

pub const VERIFY_PER_SITE: Quota = Quota {
    bucket: "verify",
    limit: 30,
    window: Duration::from_secs(3600),
};

/// 计数并判断是否超限；Redis 只用来看趋势，挂了就放行。
pub async fn charge(state: &AppState, key: &str, quota: Quota) -> Result<()> {
    let slot = format!("ic:quota:{}:{}", quota.bucket, key);
    let mut conn = state.redis().clone();
    let count: i64 = match conn.incr(&slot, 1i64).await {
        Ok(count) => count,
        Err(_) => return Ok(()),
    };
    if count == 1 {
        let _: anyhow::Result<bool> = conn
            .expire(&slot, quota.window.as_secs() as i64)
            .await
            .context("set quota ttl");
    }
    if count > quota.limit {
        return Err(Problem::code(
            Code::RateLimited,
            format!(
                "{} limit {} per {}s",
                quota.bucket,
                quota.limit,
                quota.window.as_secs()
            ),
        ));
    }
    Ok(())
}

/// 站点维度计量用 site_key，账号维度用 account id。
#[cfg(test)]
mod tests {
    use super::VERIFY_PER_SITE;

    #[test]
    fn verify_quota_is_hourly() {
        assert_eq!(VERIFY_PER_SITE.window.as_secs(), 3600);
        assert_eq!(VERIFY_PER_SITE.bucket, "verify");
    }
}
