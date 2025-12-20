//! 服务端主逻辑

use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::fs;
use std::sync::{Arc, RwLock};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{accept_async, connect_async, connect_async_tls_with_config, tungstenite::Message, Connector};
use tokio_rustls::TlsAcceptor;
use native_tls::TlsConnector as NativeTlsConnector;
use rustls::{ServerConfig as RustlsConfig, server::ServerSessionMemoryCache};
use rustls_pemfile::{certs, pkcs8_private_keys};
use tracing::{info, error, warn};
use futures_util::{SinkExt, StreamExt};
use socket2::{Socket, Domain, Type, Protocol, SockRef};

use crate::config::Config;
use crate::proxy::run_proxy;

/// 认证消息（唯一需要解析的）
#[derive(Debug, Deserialize)]
struct AuthMessage {
    token: String,
    target: String,  // 目标 WebSocket URL
}

/// Relay 服务器
pub struct RelayServer {
    config: Arc<RwLock<Config>>,
    config_path: String,
    tls_acceptor: Option<TlsAcceptor>,
}

impl RelayServer {
    pub fn new(config: Config, config_path: String) -> Result<Self> {
        // 根据配置决定是否加载 TLS
        let tls_acceptor = if config.server.enable_tls {
            info!("TLS 已启用");
            Some(Self::load_tls_config(&config)?)
        } else {
            info!("TLS 已禁用 (由反向代理处理)");
            None
        };

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
            tls_acceptor,
        })
    }

    /// 加载 TLS 配置
    fn load_tls_config(config: &Config) -> Result<TlsAcceptor> {
        let cert_path = config.server.tls_cert.as_ref()
            .ok_or(anyhow!("enable_tls=true 但未配置 tls_cert"))?;
        let key_path = config.server.tls_key.as_ref()
            .ok_or(anyhow!("enable_tls=true 但未配置 tls_key"))?;

        info!("正在加载 TLS 证书: {}", cert_path);
        info!("正在加载 TLS 私钥: {}", key_path);

        // 读取证书
        let cert_file = fs::File::open(cert_path)?;
        let mut cert_reader = std::io::BufReader::new(cert_file);
        let certs: Vec<_> = certs(&mut cert_reader)
            .collect::<Result<_, _>>()?;

        // 读取私钥
        let key_file = fs::File::open(key_path)?;
        let mut key_reader = std::io::BufReader::new(key_file);
        let keys = pkcs8_private_keys(&mut key_reader)
            .collect::<Result<Vec<_>, _>>()?;

        if keys.is_empty() {
            return Err(anyhow!("未找到私钥: {}", key_path));
        }

        // 创建 TLS 配置（启用 Session Resumption）
        let mut tls_config = RustlsConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, keys[0].clone_key().into())?;

        // 启用 Session Resumption（TLS 1.3 0-RTT）
        tls_config.session_storage = ServerSessionMemoryCache::new(1024);
        if let Ok(ticketer) = rustls::crypto::ring::Ticketer::new() {
            tls_config.ticketer = ticketer;
            info!("✓ TLS Session Resumption 已启用");
        } else {
            warn!("无法启用 TLS Session Resumption");
        }

        Ok(TlsAcceptor::from(Arc::new(tls_config)))
    }

    /// 启动热更新监听（SIGHUP 信号）
    fn start_hot_reload_watcher(&self) {
        let config = self.config.clone();
        let config_path = self.config_path.clone();

        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};

                let mut sighup = match signal(SignalKind::hangup()) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("无法监听 SIGHUP 信号: {}", e);
                        return;
                    }
                };

                loop {
                    sighup.recv().await;
                    info!("正在重新加载配置");

                    match Config::load(&config_path) {
                        Ok(new_config) => {
                            let user_count = new_config.users.len();
                            *config.write().expect("配置锁中毒") = new_config;
                            info!("配置重新加载成功，用户数: {}", user_count);
                        }
                        Err(e) => {
                            error!("配置重新加载失败，保持旧配置: {}", e);
                        }
                    }
                }
            }

            #[cfg(not(unix))]
            {
                warn!("当前平台不支持 SIGHUP 热更新");
            }
        });
    }

    /// 启动服务器
    pub async fn run(&self) -> Result<()> {
        let config_read = self.config.read().expect("配置锁中毒");
        let addr = format!("{}:{}", config_read.server.host, config_read.server.port);
        drop(config_read);

        // 启动热更新监听
        self.start_hot_reload_watcher();

        // 创建优化的 TCP listener
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;

        // 设置 socket 选项
        socket.set_reuse_address(true)?;

        // 启用 TCP Fast Open (Linux) - 使用原始系统调用
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();
            unsafe {
                let queue_len: libc::c_int = 128;
                let ret = libc::setsockopt(
                    fd,
                    libc::IPPROTO_TCP,
                    23, // TCP_FASTOPEN
                    &queue_len as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&queue_len) as libc::socklen_t,
                );
                if ret == 0 {
                    info!("✓ TCP Fast Open 已启用");
                } else {
                    warn!("无法启用 TCP Fast Open");
                }
            }
        }

        socket.bind(&addr.parse::<std::net::SocketAddr>()?.into())?;
        socket.listen(128)?;
        socket.set_nonblocking(true)?;

        let listener = TcpListener::from_std(socket.into())?;

        if self.tls_acceptor.is_some() {
            info!("中继服务器启动成功: {} (TLS 已启用, wss://)", addr);
        } else {
            info!("中继服务器启动成功: {} (TLS 已禁用, ws://)", addr);
        }

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("新连接来自: {}", addr);

                    // TCP 优化
                    Self::optimize_tcp_socket(&stream)?;

                    let config = self.config.clone();
                    let tls_acceptor = self.tls_acceptor.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(stream, config, tls_acceptor).await {
                            error!("连接错误 {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("接受连接失败: {}", e);
                }
            }
        }
    }

    /// 优化 TCP Socket
    fn optimize_tcp_socket(stream: &TcpStream) -> Result<()> {
        // 1. TCP_NODELAY - 禁用 Nagle 算法（必须）
        stream.set_nodelay(true)?;

        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            let fd = stream.as_raw_fd();
            unsafe {
                // 2. TCP_QUICKACK - 快速 ACK，减少延迟
                let optval: libc::c_int = 1;
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_TCP,
                    libc::TCP_QUICKACK,
                    &optval as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&optval) as libc::socklen_t,
                );

                // 3. SO_PRIORITY - 高优先级
                let priority: libc::c_int = 6;
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_PRIORITY,
                    &priority as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&priority) as libc::socklen_t,
                );
            }
        }

        // 4. 设置接收/发送缓冲区
        let sock_ref = SockRef::from(stream);
        let _ = sock_ref.set_recv_buffer_size(262144); // 256KB
        let _ = sock_ref.set_send_buffer_size(262144); // 256KB

        Ok(())
    }

    /// 创建 TLS Connector（用于连接目标服务器）
    fn create_tls_connector(insecure_skip_verify: bool) -> Result<Option<Connector>> {
        if !insecure_skip_verify {
            // 使用默认的 TLS 验证
            return Ok(None);
        }

        // 跳过证书验证（仅测试环境）
        let mut builder = NativeTlsConnector::builder();
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
        let tls_connector = builder.build()?;
        Ok(Some(Connector::NativeTls(tls_connector)))
    }

    /// 处理单个连接
    async fn handle_connection(
        stream: TcpStream,
        config: Arc<RwLock<Config>>,
        tls_acceptor: Option<TlsAcceptor>,
    ) -> Result<()> {
        // 根据配置决定是否需要 TLS
        if let Some(acceptor) = tls_acceptor {
            let tls_stream = acceptor.accept(stream).await?;
            Self::process_websocket(tls_stream, config).await
        } else {
            Self::process_websocket(stream, config).await
        }
    }

    /// 处理 WebSocket 连接逻辑（泛型实现）
    async fn process_websocket<S>(stream: S, config: Arc<RwLock<Config>>) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let mut client_ws = accept_async(stream).await?;

        // 1. 读取认证消息（带超时）
        let auth_timeout = {
            let cfg = config.read().expect("配置锁中毒");
            Duration::from_secs(cfg.server.auth_timeout_secs)
        };

        let auth_msg = timeout(auth_timeout, client_ws.next())
            .await
            .map_err(|_| anyhow!("认证超时"))?
            .ok_or(anyhow!("未收到认证消息"))??;

        let auth: AuthMessage = serde_json::from_slice(&auth_msg.into_data())?;

        // 2. 验证 Token（从用户列表中查找）
        let user = {
            let cfg = config.read().expect("配置锁中毒");
            cfg.users.iter()
                .find(|u| u.token == auth.token)
                .cloned()
        };

        let user = match user {
            Some(u) => {
                info!("用户认证成功: {}", u.name);
                u
            }
            None => {
                warn!("认证失败: token 无效");
                client_ws.send(Message::text(r#"{"error":"认证失败"}"#)).await?;
                return Ok(());
            }
        };

        // 3. 连接目标 WebSocket（快速失败）
        info!("[{}] 正在连接目标: {}", user.name, auth.target);

        let connector = {
            let cfg = config.read().expect("配置锁中毒");
            Self::create_tls_connector(cfg.server.insecure_skip_verify)?
        };

        let target_result = match connector {
            Some(conn) => connect_async_tls_with_config(&auth.target, None, false, Some(conn)).await,
            None => connect_async(&auth.target).await,
        };

        let (target_ws, _) = match target_result {
            Ok(ws) => ws,
            Err(e) => {
                error!("[{}] 连接目标失败: {}", user.name, e);
                let err_msg = format!(r#"{{"error":"连接失败: {}"}}"#, e);
                client_ws.send(Message::text(err_msg)).await?;
                return Ok(());
            }
        };

        // 4. 返回成功
        client_ws.send(Message::text(r#"{"status":"已连接"}"#)).await?;

        // 5. 启动双向透传（带空闲超时配置）
        let (idle_timeout_secs, timeout_display) = {
            let cfg = config.read().expect("配置锁中毒");
            let timeout = cfg.server.idle_timeout_secs;
            let display = if timeout > 0 {
                format!("{}s", timeout)
            } else {
                "禁用".to_string()
            };
            (timeout, display)
        };

        info!("[{}] 启动代理: {} (空闲超时: {})", user.name, auth.target, timeout_display);
        run_proxy(client_ws, target_ws, idle_timeout_secs).await?;

        info!("[{}] 代理会话结束", user.name);
        Ok(())
    }
}
