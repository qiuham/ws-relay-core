//! 认证中间件

use axum::{
    extract::{Query, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use serde::Deserialize;
use std::{collections::HashSet, sync::Arc};

use crate::config::User;

/// 认证状态
#[derive(Clone)]
pub struct AuthState {
    tokens: Arc<HashSet<String>>,
}

impl AuthState {
    pub fn new(users: &[User]) -> Self {
        Self {
            tokens: Arc::new(users.iter().map(|u| u.token.clone()).collect()),
        }
    }
}

/// Query 参数
#[derive(Deserialize, Default)]
pub struct TokenQuery {
    token: Option<String>,
}

/// 认证中间件
/// 从 Query(?token=xxx) 或 Header(X-Token: xxx) 提取 token
pub async fn middleware(
    State(state): State<AuthState>,
    Query(query): Query<TokenQuery>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // header 优先（REST 常用），其次 query（WS 常用）
    let token = req
        .headers()
        .get("x-token")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .or(query.token);

    match token {
        Some(ref t) if state.tokens.contains(t) => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
