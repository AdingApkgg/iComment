//! 内容寻址的媒体存储（CAS）：文件名就是内容的 sha256，同样的图只存一份。
//!
//! 引用关系由 `attachment.hash` 表示，没人引用超过一天的 blob 由清理任务删。
//! 字节落 SeaweedFS 的 filer（`IC_SEAWEED_FILER_URL`）或本地目录（`IC_UPLOAD_DIR`），
//! 目录显式留空时退到进程内存（冒烟用）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
    upload,
};

/// 没人引用的 blob 留一天再删，给"传了但没发评论"留改正余地。
const ORPHAN_AGE_SECS: i64 = 86_400;

#[derive(Debug, Clone)]
pub enum Backend {
    /// 进程内存，跨用例不共享
    Memory,
    Fs(PathBuf),
    Seaweed {
        base: String,
    },
}

impl Backend {
    /// 没配本地目录时就是内存；上传接口是否开放由 `Config::uploads_enabled` 决定。
    pub fn from_cfg(cfg: &crate::config::Config) -> Self {
        if let Some(base) = cfg.seaweed_filer_url.as_deref() {
            let base = base.trim_end_matches('/');
            if !base.is_empty() {
                return Self::Seaweed {
                    base: base.to_owned(),
                };
            }
        }
        if cfg.upload_dir.trim().is_empty() {
            return Self::Memory;
        }
        Self::Fs(PathBuf::from(&cfg.upload_dir))
    }

    async fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        match self {
            Self::Memory => {
                if let Ok(mut m) = memory().lock() {
                    m.insert(key.to_owned(), bytes.to_vec());
                }
                Ok(())
            }
            Self::Fs(dir) => {
                tokio::fs::create_dir_all(dir)
                    .await
                    .map_err(|e| Problem::code(Code::Internal, format!("blob dir: {e}")))?;
                tokio::fs::write(dir.join(key), bytes)
                    .await
                    .map_err(|e| Problem::code(Code::Internal, format!("blob write: {e}")))
            }
            Self::Seaweed { base } => {
                // filer 只认 PUT（或 multipart POST）写文件，POST 裸体会回 500
                let resp = http()?
                    .put(format!("{base}/{key}"))
                    .header(reqwest::header::CONTENT_TYPE, upload::content_type(key))
                    .body(bytes.to_vec())
                    .send()
                    .await
                    .map_err(|e| Problem::code(Code::Internal, format!("filer put: {e}")))?;
                if !resp.status().is_success() {
                    return Err(Problem::code(
                        Code::Internal,
                        format!("filer put: {}", resp.status()),
                    ));
                }
                Ok(())
            }
        }
    }

    /// 取字节；不存在时 None。
    async fn get(&self, key: &str) -> Option<Vec<u8>> {
        match self {
            Self::Memory => memory().lock().ok()?.get(key).cloned(),
            Self::Fs(dir) => tokio::fs::read(dir.join(key)).await.ok(),
            Self::Seaweed { base } => {
                let resp = http()
                    .ok()?
                    .get(format!("{base}/{key}"))
                    .send()
                    .await
                    .ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                resp.bytes().await.ok().map(|b| b.to_vec())
            }
        }
    }

    async fn remove(&self, key: &str) -> Result<()> {
        match self {
            Self::Memory => {
                if let Ok(mut m) = memory().lock() {
                    m.remove(key);
                }
                Ok(())
            }
            Self::Fs(dir) => match tokio::fs::remove_file(dir.join(key)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(Problem::code(Code::Internal, format!("blob unlink: {e}"))),
            },
            Self::Seaweed { base } => {
                let resp = http()?
                    .delete(format!("{base}/{key}"))
                    .send()
                    .await
                    .map_err(|e| Problem::code(Code::Internal, format!("filer delete: {e}")))?;
                // 404 表示已经没了，不算错
                if resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND {
                    Ok(())
                } else {
                    Err(Problem::code(
                        Code::Internal,
                        format!("filer delete: {}", resp.status()),
                    ))
                }
            }
        }
    }
}

fn memory() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    static MEM: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    MEM.get_or_init(|| Mutex::new(HashMap::new()))
}

fn http() -> Result<reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(3))
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .ok()
        })
        .clone()
        .ok_or_else(|| Problem::code(Code::Internal, "http client unavailable"))
}

/// 小写十六进制 sha256。
pub fn key_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 存一份内容，返回 `hash`；已存在就不重复写字节。
pub async fn put(state: &AppState, ext: &str, bytes: &[u8]) -> Result<String> {
    let hash = key_of(bytes);
    let known: bool = sqlx::query_scalar("select exists (select 1 from blob where hash = $1)")
        .bind(&hash)
        .fetch_one(state.db())
        .await?;
    if known {
        return Ok(hash);
    }
    Backend::from_cfg(state.cfg())
        .put(&format!("{hash}.{ext}"), bytes)
        .await?;
    sqlx::query(
        "insert into blob (hash, bytes, ext) values ($1, $2, $3) on conflict (hash) do nothing",
    )
    .bind(&hash)
    .bind(bytes.len() as i32)
    .bind(ext)
    .execute(state.db())
    .await?;
    Ok(hash)
}

/// 读一份内容；没这条记录就不存在。
pub async fn get(state: &AppState, hash: &str) -> Option<(Vec<u8>, &'static str)> {
    let ext: Option<String> = sqlx::query_scalar("select ext from blob where hash = $1")
        .bind(hash)
        .fetch_optional(state.db())
        .await
        .ok()?;
    let ext = ext?;
    let bytes = Backend::from_cfg(state.cfg())
        .get(&format!("{hash}.{ext}"))
        .await?;
    Some((bytes, upload::content_type(&ext)))
}

/// 删掉没人引用且放够一天的行，顺带删字节。
pub async fn sweep_orphans(state: &AppState) -> Result<u64> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "delete from blob b where b.created_at < now() - ($1 || ' seconds')::interval \
         and not exists (select 1 from attachment a where a.hash = b.hash) \
         returning hash, ext",
    )
    .bind(ORPHAN_AGE_SECS.to_string())
    .fetch_all(state.db())
    .await?;
    let backend = Backend::from_cfg(state.cfg());
    for (hash, ext) in &rows {
        if let Err(e) = backend.remove(&format!("{hash}.{ext}")).await {
            tracing::warn!(error = %e, hash, "blob delete failed");
        }
    }
    Ok(rows.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_bytes_same_key() {
        let a = key_of(b"hello");
        assert_eq!(a, key_of(b"hello"));
        assert_ne!(a, key_of(b"hell"));
        assert_eq!(a.len(), 64);
        assert_eq!(
            a,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
