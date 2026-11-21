//! 账号：只用于网页登录（建站/审核/取 token）。agent 侧不碰密码。

use uuid::Uuid;

use crate::{
    error::{Code, Problem, Result},
    state::AppState,
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Account {
    pub id: Uuid,
    pub email: String,
    pub status: String,
}

pub async fn ensure(state: &AppState, email: &str) -> Result<Account> {
    let email = email.trim().to_lowercase();
    if !email.contains('@') || email.len() > 254 {
        return Err(Problem::field("email", "not an email"));
    }

    let existing: Option<Account> = sqlx::query_as("select * from account where email = $1")
        .bind(&email)
        .fetch_optional(state.db())
        .await?;
    if let Some(account) = existing {
        if account.status != "active" {
            return Err(Problem::code(Code::Forbidden, "account suspended"));
        }
        return Ok(account);
    }

    let account: Account =
        sqlx::query_as("insert into account (id, email) values ($1, $2) returning *")
            .bind(Uuid::now_v7())
            .bind(&email)
            .fetch_one(state.db())
            .await?;
    Ok(account)
}

/// 第三方登录：同一个 (provider, subject) 永远回到同一个账号；
/// 首次登录时邮箱若已存在就并入，否则新建。
pub async fn login_with_identity(
    state: &AppState,
    provider: &str,
    subject: &str,
    email: &str,
) -> Result<Account> {
    let email = email.trim().to_lowercase();
    if !email.contains('@') || email.len() > 254 {
        return Err(Problem::field("email", "not an email"));
    }

    if let Some(account) = by_identity(state, provider, subject).await? {
        if account.status != "active" {
            return Err(Problem::code(Code::Forbidden, "account suspended"));
        }
        return Ok(account);
    }

    let mut tx = state.db().begin().await?;

    let account: Account = match sqlx::query_as("select * from account where email = $1")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?
    {
        Some(account) => account,
        None => {
            sqlx::query_as("insert into account (id, email) values ($1, $2) returning *")
                .bind(Uuid::now_v7())
                .bind(&email)
                .fetch_one(&mut *tx)
                .await?
        }
    };
    if account.status != "active" {
        tx.rollback().await?;
        return Err(Problem::code(Code::Forbidden, "account suspended"));
    }

    let linked: Option<Uuid> = sqlx::query_scalar(
        "insert into account_identity (id, account_id, provider, subject, email) \
         values ($1, $2, $3, $4, $5) \
         on conflict (provider, subject) do nothing returning account_id",
    )
    .bind(Uuid::now_v7())
    .bind(account.id)
    .bind(provider)
    .bind(subject)
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;

    // 并发首登：另一个请求已经绑过了，用它的账号，别把刚建的空号留下
    if linked.is_none() {
        tx.rollback().await?;
        return by_identity(state, provider, subject)
            .await?
            .ok_or_else(|| Problem::code(Code::Internal, "identity vanished"));
    }

    tx.commit().await?;
    Ok(account)
}

async fn by_identity(state: &AppState, provider: &str, subject: &str) -> Result<Option<Account>> {
    let account: Option<Account> = sqlx::query_as(
        "select a.* from account a \
         join account_identity i on i.account_id = a.id \
         where i.provider = $1 and i.subject = $2",
    )
    .bind(provider)
    .bind(subject)
    .fetch_optional(state.db())
    .await?;
    Ok(account)
}

/// 这个账号绑过哪些 provider，登录页/后台用来提示。
pub async fn identities(state: &AppState, account_id: Uuid) -> Result<Vec<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "select provider from account_identity where account_id = $1 order by created_at",
    )
    .bind(account_id)
    .fetch_all(state.db())
    .await?;
    Ok(rows)
}

pub async fn by_id(state: &AppState, id: Uuid) -> Result<Account> {
    let account: Option<Account> = sqlx::query_as("select * from account where id = $1")
        .bind(id)
        .fetch_optional(state.db())
        .await?;
    account.ok_or_else(|| Problem::code(Code::Unauthenticated, "account gone"))
}
