//! 统一错误：RFC 9457 `application/problem+json`，`code` 是封闭枚举，供 agent 分支。

use std::borrow::Cow;

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

/// 封闭错误码。agent 靠它分支，不靠 title 文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    // 认证
    Unauthenticated,
    Forbidden,
    DevicePending,
    DeviceExpired,
    // 输入
    Validation,
    SiteNotFound,
    ThreadNotFound,
    CommentNotFound,
    NotFound,
    // 反垃圾
    ChallengeRequired,
    ChallengeFailed,
    TooLarge,
    RateLimited,
    // 运维
    Upstream,
    Internal,
}

impl Code {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::Forbidden => "forbidden",
            Self::DevicePending => "device_authorization_pending",
            Self::DeviceExpired => "device_code_expired",
            Self::Validation => "validation_failed",
            Self::SiteNotFound => "site_not_found",
            Self::ThreadNotFound => "thread_not_found",
            Self::CommentNotFound => "comment_not_found",
            Self::NotFound => "not_found",
            Self::ChallengeRequired => "challenge_required",
            Self::ChallengeFailed => "challenge_failed",
            Self::TooLarge => "payload_too_large",
            Self::RateLimited => "rate_limited",
            Self::Upstream => "upstream_failed",
            Self::Internal => "internal",
        }
    }

    pub const fn status(self) -> StatusCode {
        match self {
            Self::Unauthenticated | Self::DevicePending | Self::DeviceExpired => {
                StatusCode::UNAUTHORIZED
            }
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Validation
            | Self::SiteNotFound
            | Self::ThreadNotFound
            | Self::CommentNotFound => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::ChallengeRequired | Self::ChallengeFailed => StatusCode::FORBIDDEN,
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Upstream => StatusCode::BAD_GATEWAY,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// agent 可直接执行的下一步。没有就给 None，避免编造。
    pub const fn next(self) -> Option<&'static str> {
        match self {
            Self::Unauthenticated => {
                Some("POST /api/v1/agent/device then poll /api/v1/agent/token")
            }
            Self::DevicePending => Some("poll POST /api/v1/agent/token again after user approves"),
            Self::DeviceExpired => Some("POST /api/v1/agent/device to start a new flow"),
            Self::SiteNotFound => Some("call site.create first (POST /api/v1/sites)"),
            Self::ThreadNotFound => {
                Some("call list_comments for this path to create/read the thread")
            }
            Self::ChallengeRequired => {
                Some("fetch a Turnstile token and resend as challenge_token")
            }
            Self::ChallengeFailed => Some("fetch a fresh Turnstile token and retry"),
            Self::RateLimited => Some("wait for the Retry-After header seconds, then retry"),
            Self::TooLarge => Some("trim body_md below 8 KiB"),
            Self::Validation => Some("read fields[] and resend with corrected values"),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum Problem {
    /// 已知可行动错误
    Code(Code, Cow<'static, str>),
    /// 校验失败，带字段级原因
    Fields(Vec<(&'static str, String)>),
    /// 兜底
    Inner(anyhow::Error),
}

pub type Result<T, E = Problem> = std::result::Result<T, E>;

impl Problem {
    pub fn code(c: Code, detail: impl Into<Cow<'static, str>>) -> Self {
        Self::Code(c, detail.into())
    }

    pub fn field(name: &'static str, reason: impl Into<String>) -> Self {
        Self::Fields(vec![(name, reason.into())])
    }

    pub fn upstream(err: impl Into<anyhow::Error>) -> Self {
        Self::Inner(err.into())
    }
}

impl From<anyhow::Error> for Problem {
    fn from(e: anyhow::Error) -> Self {
        Self::Inner(e)
    }
}

impl From<sqlx::Error> for Problem {
    fn from(e: sqlx::Error) -> Self {
        Self::Inner(e.into())
    }
}

impl From<redis::RedisError> for Problem {
    fn from(e: redis::RedisError) -> Self {
        Self::Inner(e.into())
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Code(c, detail) => write!(f, "{}: {detail}", c.as_str()),
            Self::Fields(fields) => {
                write!(f, "validation_failed: ")?;
                for (name, reason) in fields {
                    write!(f, "{name}={reason}; ")?;
                }
                Ok(())
            }
            Self::Inner(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for Problem {}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let (code, detail, fields): (Code, String, Option<Vec<(&'static str, String)>>) = match self
        {
            Self::Code(c, detail) => (c, detail.into_owned(), None),
            Self::Fields(fields) => {
                let detail = fields
                    .iter()
                    .map(|(n, r)| format!("{n}: {r}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                (Code::Validation, detail, Some(fields))
            }
            Self::Inner(e) => {
                let code = if e.to_string().contains("decode") {
                    Code::Validation
                } else {
                    Code::Internal
                };
                tracing::warn!(error = %format!("{e:#}"), "internal error");
                (code, format!("internal error: {e:#}"), None)
            }
        };

        let mut body = serde_json::json!({
            "type": format!("https://icomment.dev/errors/{}", code.as_str()),
            "code": code.as_str(),
            "title": code.as_str().replace('_', " "),
            "detail": detail,
        });
        if let Some(next) = code.next() {
            body["next"] = serde_json::Value::from(next);
        }
        if let Some(fields) = fields {
            body["fields"] = serde_json::Value::from(
                fields
                    .into_iter()
                    .map(|(n, r)| serde_json::json!({ "field": n, "reason": r }))
                    .collect::<Vec<_>>(),
            );
        }

        let mut res = (code.status(), Json(body)).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if code == Code::RateLimited {
            res.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        }
        res
    }
}
