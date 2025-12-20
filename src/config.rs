//! 配置模块

use serde::Deserialize;
use anyhow::{Result, anyhow};
use std::fs;
use std::collections::HashSet;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    pub users: Vec<User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub name: String,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_directory")]
    pub directory: String,
    #[serde(default = "default_log_prefix")]
    pub file_prefix: String,
    #[serde(default = "default_log_rotation")]
    pub rotation: String,
    #[serde(default = "default_console_output")]
    pub console_output: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            directory: default_log_directory(),
            file_prefix: default_log_prefix(),
            rotation: default_log_rotation(),
            console_output: default_console_output(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_enable_tls")]
    pub enable_tls: bool,

    #[serde(default)]
    pub tls_cert: Option<String>,
    #[serde(default)]
    pub tls_key: Option<String>,

    #[serde(default = "default_auth_timeout")]
    pub auth_timeout_secs: u64,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,

    #[serde(default)]
    pub insecure_skip_verify: bool,
}

// 默认值
fn default_host() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 443 }
fn default_enable_tls() -> bool { true }
fn default_auth_timeout() -> u64 { 10 }
fn default_idle_timeout() -> u64 { 600 }

fn default_log_level() -> String { "info".to_string() }
fn default_log_directory() -> String { "logs".to_string() }
fn default_log_prefix() -> String { "ws-relay".to_string() }
fn default_log_rotation() -> String { "daily".to_string() }
fn default_console_output() -> bool { true }

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;

        // 校验配置
        config.validate()?;

        Ok(config)
    }

    /// 校验配置有效性
    fn validate(&self) -> Result<()> {
        // 1. 用户列表不能为空
        if self.users.is_empty() {
            return Err(anyhow!("配置错误: users 不能为空，至少需要配置一个用户"));
        }

        // 2. 检查 token 重复
        let mut tokens = HashSet::new();
        for user in &self.users {
            if !tokens.insert(&user.token) {
                return Err(anyhow!("配置错误: 用户 '{}' 的 token 重复", user.name));
            }
        }

        // 3. 检查用户名不能为空
        for user in &self.users {
            if user.name.trim().is_empty() {
                return Err(anyhow!("配置错误: 用户名不能为空"));
            }
            if user.token.trim().is_empty() {
                return Err(anyhow!("配置错误: 用户 '{}' 的 token 不能为空", user.name));
            }
        }

        Ok(())
    }
}
