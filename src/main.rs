//! ws-relay-core - 高性能 WebSocket 中继

mod config;
mod proxy;
mod server;

use anyhow::{Result, anyhow};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tracing_appender::rolling;
use std::fs;

use crate::config::Config;
use crate::server::RelayServer;

const PIDFILE: &str = "/tmp/ws-relay.pid";

/// PID 文件守卫（RAII 模式，自动清理）
struct PidFileGuard;

impl PidFileGuard {
    fn create() -> Result<Self> {
        let pid = std::process::id();
        fs::write(PIDFILE, pid.to_string())?;
        info!("PID 文件已创建: {} (pid={})", PIDFILE, pid);
        Ok(Self)
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(PIDFILE);
        info!("PID 文件已清理");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 检查是否是 reload 命令
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "reload" {
        return handle_reload();
    }

    // 安装 rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // 加载配置
    let config_path = args.get(1)
        .map(|s| s.as_str())
        .unwrap_or("config.toml");

    let config = Config::load(config_path)?;

    // 初始化日志（带轮转）
    init_logging(&config)?;

    info!("Starting ws-relay v{}", env!("CARGO_PKG_VERSION"));
    info!("Config loaded from {}", config_path);
    info!("用户数: {}", config.users.len());

    // 创建 pidfile（RAII 自动清理）
    let _pid_guard = PidFileGuard::create()?;

    // 启动服务器
    let server = RelayServer::new(config, config_path.to_string())?;
    server.run().await
}

/// 处理 reload 命令
fn handle_reload() -> Result<()> {
    #[cfg(unix)]
    {
        // 读取 pidfile
        let pid_str = fs::read_to_string(PIDFILE)
            .map_err(|_| anyhow!("无法读取 pidfile: {}，服务器可能未运行", PIDFILE))?;

        let pid: i32 = pid_str.trim().parse()
            .map_err(|_| anyhow!("pidfile 格式错误"))?;

        // 验证进程是否存在
        unsafe {
            if libc::kill(pid, 0) != 0 {
                // 进程不存在，清理残留的 PID 文件
                let _ = fs::remove_file(PIDFILE);
                return Err(anyhow!("重载失败: 进程不存在（已清理残留 PID 文件）"));
            }
        }

        // 发送 SIGHUP 信号
        unsafe {
            if libc::kill(pid, libc::SIGHUP) == 0 {
                println!("配置重载成功");
                Ok(())
            } else {
                Err(anyhow!("重载失败: 发送信号失败"))
            }
        }
    }

    #[cfg(not(unix))]
    {
        Err(anyhow!("reload 命令仅支持 Unix 系统"))
    }
}

/// 初始化日志系统（带轮转）
fn init_logging(config: &Config) -> Result<()> {
    use std::fs;

    // 创建日志目录
    fs::create_dir_all(&config.logging.directory)?;

    // 创建文件 appender（带轮转）
    let file_appender = match config.logging.rotation.as_str() {
        "daily" => rolling::daily(&config.logging.directory, &config.logging.file_prefix),
        "hourly" => rolling::hourly(&config.logging.directory, &config.logging.file_prefix),
        "never" => rolling::never(&config.logging.directory, &config.logging.file_prefix),
        _ => rolling::daily(&config.logging.directory, &config.logging.file_prefix),
    };

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // 配置日志等级
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.logging.level))?;

    // 文件输出层
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    // 构建订阅器
    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer);

    if config.logging.console_output {
        // 同时输出到控制台
        registry.with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout)).init();
    } else {
        registry.init();
    }

    // 防止 _guard 被释放
    std::mem::forget(_guard);

    Ok(())
}
