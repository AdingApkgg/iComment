//! OAuth 登录：Google 与 GitHub。
//!
//! 只做「拿一个可核验的邮箱 + 稳定 subject」这一件事，会话仍走 web 层的签名 cookie。
//! 凭据全在环境变量里，没配的 provider 在登录页不出现。

use reqwest::Url;
use serde::Deserialize;

use crate::{
    config::{Config, OAuthApp},
    error::{Code, Problem, Result},
    state::AppState,
};

/// state 参数落在短命 cookie 里，和回调对上才继续。
pub const STATE_COOKIE: &str = "ic_oauth";
pub const STATE_TTL_SECS: i64 = 600;
const HTTP_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Google,
    GitHub,
}

impl Provider {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "google" => Some(Self::Google),
            "github" => Some(Self::GitHub),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::GitHub => "github",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::GitHub => "GitHub",
        }
    }

    fn app(self, cfg: &Config) -> Option<&OAuthApp> {
        match self {
            Self::Google => cfg.google.as_ref(),
            Self::GitHub => cfg.github.as_ref(),
        }
    }

    fn authorize_endpoint(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::GitHub => "https://github.com/login/oauth/authorize",
        }
    }

    fn token_endpoint(self) -> &'static str {
        match self {
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::GitHub => "https://github.com/login/oauth/access_token",
        }
    }

    /// GitHub 要 `user:email` 才能读到已验证邮箱。
    fn scope(self) -> &'static str {
        match self {
            Self::Google => "openid email profile",
            Self::GitHub => "read:user user:email",
        }
    }
}

/// 配好了凭据的 provider，按展示顺序。
pub fn available(cfg: &Config) -> Vec<Provider> {
    [Provider::Google, Provider::GitHub]
        .into_iter()
        .filter(|p| p.app(cfg).is_some())
        .collect()
}

pub fn callback_uri(cfg: &Config, provider: Provider) -> String {
    format!(
        "{}/auth/{}/callback",
        cfg.public_base_url,
        provider.as_str()
    )
}

/// 拼授权地址。`state` 是随机串，回调时比对 cookie。
pub fn authorize_url(
    cfg: &Config,
    provider: Provider,
    redirect_uri: &str,
    state: &str,
) -> Result<String> {
    let app = provider.app(cfg).ok_or_else(|| {
        Problem::code(
            Code::NotFound,
            format!("{} login is not configured", provider.as_str()),
        )
    })?;

    let mut url = Url::parse(provider.authorize_endpoint())
        .map_err(|err| Problem::code(Code::Internal, err.to_string()))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", &app.client_id);
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("response_type", "code");
        q.append_pair("scope", provider.scope());
        q.append_pair("state", state);
        if provider == Provider::Google {
            // 每次都要选账号，避免用错身份建站
            q.append_pair("prompt", "select_account");
        }
    }
    Ok(url.into())
}

/// 用户信息里我们真正要的两样。
#[derive(Debug, Clone)]
pub struct Profile {
    pub subject: String,
    pub email: String,
}

/// 换 token 再读 profile；两步都走同一套超时与错误映射。
pub async fn exchange_and_profile(
    state: &AppState,
    provider: Provider,
    code: &str,
    redirect_uri: &str,
) -> Result<Profile> {
    let app = provider.app(state.cfg()).ok_or_else(|| {
        Problem::code(
            Code::NotFound,
            format!("{} login is not configured", provider.as_str()),
        )
    })?;
    let token = access_token(state, provider, app, code, redirect_uri).await?;
    profile(state, provider, &token).await
}

async fn access_token(
    state: &AppState,
    provider: Provider,
    app: &OAuthApp,
    code: &str,
    redirect_uri: &str,
) -> Result<String> {
    let form = [
        ("client_id", app.client_id.as_str()),
        ("client_secret", app.client_secret.as_str()),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];
    let resp = state
        .http()
        .post(provider.token_endpoint())
        .header("accept", "application/json")
        .form(&form)
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|err| Problem::code(Code::Upstream, format!("token exchange failed: {err}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|err| Problem::code(Code::Upstream, format!("token body: {err}")))?;
    if !status.is_success() {
        return Err(Problem::code(
            Code::Upstream,
            format!("token endpoint returned {status}"),
        ));
    }

    let parsed: TokenResponse = serde_json::from_str(&body)
        .map_err(|_| Problem::code(Code::Upstream, "token endpoint returned non-json"))?;
    parsed
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Problem::code(Code::Upstream, "token endpoint returned no access_token"))
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
}

async fn profile(state: &AppState, provider: Provider, token: &str) -> Result<Profile> {
    match provider {
        Provider::Google => google_profile(state, token).await,
        Provider::GitHub => github_profile(state, token).await,
    }
}

#[derive(Deserialize)]
struct GoogleUser {
    sub: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
}

async fn google_profile(state: &AppState, token: &str) -> Result<Profile> {
    let user: GoogleUser = state
        .http()
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|err| Problem::code(Code::Upstream, format!("userinfo failed: {err}")))?
        .error_for_status()
        .map_err(|err| Problem::code(Code::Upstream, format!("userinfo status: {err}")))?
        .json()
        .await
        .map_err(|err| Problem::code(Code::Upstream, format!("userinfo body: {err}")))?;

    let email = user
        .email
        .filter(|e| !e.trim().is_empty())
        .ok_or_else(|| Problem::code(Code::Upstream, "google account has no email"))?;
    if !user.email_verified {
        return Err(Problem::code(
            Code::Forbidden,
            "google email is not verified",
        ));
    }
    Ok(Profile {
        subject: user.sub,
        email,
    })
}

#[derive(Deserialize)]
struct GithubUser {
    id: u64,
    email: Option<String>,
}

#[derive(Deserialize)]
struct GithubEmail {
    email: String,
    primary: bool,
    verified: bool,
}

async fn github_profile(state: &AppState, token: &str) -> Result<Profile> {
    let user: GithubUser = state
        .http()
        .get("https://api.github.com/user")
        .bearer_auth(token)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "icomment-core")
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|err| Problem::code(Code::Upstream, format!("user failed: {err}")))?
        .error_for_status()
        .map_err(|err| Problem::code(Code::Upstream, format!("user status: {err}")))?
        .json()
        .await
        .map_err(|err| Problem::code(Code::Upstream, format!("user body: {err}")))?;

    // 公开邮箱可能是空的，也可能没验证；缺了就单独拉一次 emails。
    let email = match user.email.filter(|e| !e.trim().is_empty()) {
        Some(email) => email,
        None => github_primary_email(state, token).await?,
    };

    Ok(Profile {
        subject: user.id.to_string(),
        email,
    })
}

async fn github_primary_email(state: &AppState, token: &str) -> Result<String> {
    let list: Vec<GithubEmail> = state
        .http()
        .get("https://api.github.com/user/emails")
        .bearer_auth(token)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "icomment-core")
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|err| Problem::code(Code::Upstream, format!("emails failed: {err}")))?
        .error_for_status()
        .map_err(|err| Problem::code(Code::Upstream, format!("emails status: {err}")))?
        .json()
        .await
        .map_err(|err| Problem::code(Code::Upstream, format!("emails body: {err}")))?;

    list.iter()
        .find(|e| e.primary && e.verified)
        .or_else(|| list.iter().find(|e| e.verified))
        .map(|e| e.email.clone())
        .ok_or_else(|| {
            Problem::code(
                Code::Forbidden,
                "github account has no verified email; verify one in GitHub settings",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(google: bool, github: bool) -> Config {
        let mut cfg = crate::config::Config {
            bind: "127.0.0.1:0".to_owned(),
            database_url: String::new(),
            redis_url: String::new(),
            public_base_url: "https://icomment.example".to_owned(),
            session_secret: "s".to_owned(),
            turnstile_secret: None,
            turnstile_site_key: None,
            akismet_key: None,
            akismet_endpoint: String::new(),
            upload_dir: String::new(),
            seaweed_filer_url: None,
            widget_cdn: String::new(),
            db_pool_max: 1,
            verify_private: false,
            cors_origins: Vec::new(),
            google: None,
            github: None,
            email_login: false,
            claim_verify: true,
        };
        if google {
            cfg.google = Some(OAuthApp {
                client_id: "gid".to_owned(),
                client_secret: "gsecret".to_owned(),
            });
        }
        if github {
            cfg.github = Some(OAuthApp {
                client_id: "ghid".to_owned(),
                client_secret: "ghsecret".to_owned(),
            });
        }
        cfg
    }

    #[test]
    fn only_configured_providers_show_up() {
        assert!(available(&cfg_with(false, false)).is_empty());
        assert_eq!(available(&cfg_with(true, false)), vec![Provider::Google]);
        assert_eq!(
            available(&cfg_with(true, true)),
            vec![Provider::Google, Provider::GitHub]
        );
    }

    #[test]
    fn authorize_url_carries_state_and_callback() {
        let cfg = cfg_with(true, true);
        let url = authorize_url(
            &cfg,
            Provider::GitHub,
            "https://x/auth/github/callback",
            "nonce",
        )
        .expect("url");
        assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(url.contains("client_id=ghid"));
        assert!(url.contains("state=nonce"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fx%2Fauth%2Fgithub%2Fcallback"));
        assert!(url.contains("scope=read%3Auser"));
        assert!(
            !url.contains("ghsecret"),
            "secret must never leave the server"
        );
    }

    #[test]
    fn unconfigured_provider_is_rejected() {
        let cfg = cfg_with(false, false);
        assert!(authorize_url(&cfg, Provider::Google, "https://x/cb", "n").is_err());
    }

    #[test]
    fn callback_uri_is_derived_from_base() {
        let cfg = cfg_with(true, true);
        assert_eq!(
            callback_uri(&cfg, Provider::Google),
            "https://icomment.example/auth/google/callback"
        );
    }
}
