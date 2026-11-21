//! `icomment-core`：自托管评论服务。
//!
//! 分层：`api`（公开 + agent）、`domain`（SQL 与状态机）、`web`（网页面）、
//! `antispam`、`markdown`、`identity`。所有错误走 `error::Problem`。

pub mod akismet;
pub mod antispam;
pub mod api;
pub mod auth;
pub mod blob;
pub mod claim;
pub mod config;
pub mod discover;
pub mod domain;
pub mod embed;
pub mod error;
pub mod extract;
pub mod identity;
pub mod markdown;
pub mod normalize;
pub mod oauth;
pub mod pages;
pub mod ratelimit;
pub mod reaction;
pub mod router;
pub mod state;
pub mod upload;
pub mod voter;
pub mod web;

pub use state::AppState;
