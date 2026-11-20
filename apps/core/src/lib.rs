//! `icomment-core`：自托管评论服务。
//!
//! 分层：`api`（公开 + agent）、`domain`（SQL 与状态机）、`web`（网页面）、
//! `antispam`、`markdown`。所有错误走 `error::Problem`。

pub mod antispam;
pub mod api;
pub mod auth;
pub mod config;
pub mod domain;
pub mod error;
pub mod extract;
pub mod markdown;
pub mod normalize;
pub mod pages;
pub mod ratelimit;
pub mod router;
pub mod state;
pub mod web;

pub use state::AppState;
