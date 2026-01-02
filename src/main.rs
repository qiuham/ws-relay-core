//! ws-relay-core - 高性能 WebSocket + REST 中继代理

mod auth;
mod config;
mod rest;
mod ws;

use anyhow::Result;
use axum::{middleware, routing::{any, get}, Router};
use axum_server::tls_rustls::RustlsConfig;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化 TLS crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // 初始化日志
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 加载配置
    let config_path = std::env::args().nth(1).unwrap_or_else(|| "config.toml".to_string());
    let config = config::Config::load(&config_path)?;

    info!("ws-relay-core v{}", env!("CARGO_PKG_VERSION"));
    info!(
        "用户: {}",
        config.users.iter().map(|u| u.name.as_str()).collect::<Vec<_>>().join(", ")
    );

    // 认证状态
    let auth_state = auth::AuthState::new(&config.users);

    // 构建路由
    let app = Router::new()
        .route("/ws/{*target}", get(ws::handler))
        .route("/rest/{*target}", any(rest::handler))
        .layer(middleware::from_fn_with_state(auth_state, auth::middleware));

    // TLS 配置
    let tls_config = RustlsConfig::from_pem_file(
        &config.server.tls_cert,
        &config.server.tls_key,
    )
    .await?;

    // 启动服务器
    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("服务启动: https://{}", addr);
    info!("WS:   /ws/<target_url>?token=xxx");
    info!("REST: /rest/<target_url> + Header: X-Token");

    axum_server::bind_rustls(addr.parse()?, tls_config)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
