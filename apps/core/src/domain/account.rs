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

pub async fn by_id(state: &AppState, id: Uuid) -> Result<Account> {
    let account: Option<Account> = sqlx::query_as("select * from account where id = $1")
        .bind(id)
        .fetch_optional(state.db())
        .await?;
    account.ok_or_else(|| Problem::code(Code::Unauthenticated, "account gone"))
}
