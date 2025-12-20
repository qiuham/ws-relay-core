# ws-relay-core

基于 Rust 的高性能 WebSocket 中继服务器。

**延迟开销仅 0.55ms (1.1%)** | **吞吐量几乎0损失** | **支持热更新**

## 性能

测试环境: 东京VPS   模拟ws服务端

**延迟测试（Ping，1000次）:**

- 直连: 48.96 ms
- 通过 ws-relay: 49.52 ms
- **延迟开销: +0.55ms (+1.1%)**

**吞吐量测试（每连接，持续10秒）:**
- 直连: 45,419 msg/s
- 通过 ws-relay: 45,416 msg/s
- **吞吐量: 100.0%（零损耗）**

## 优势

1. **极低延迟** - 相比直连仅增加 0.55ms 延迟
2. **零拷贝转发** - 使用 `futures::forward()` 批量转发，无额外开销
3. **无需重启** - 修改用户配置后 reload 即可生效
4. **高并发** - 基于 Tokio 异步运行时，支持数千并发连接
5. **TCP 优化** - 启用 TCP_NODELAY、TCP_QUICKACK、TCP_FASTOPEN
6. **TLS 优化** - 支持 Session Resumption (TLS 1.3 0-RTT)

## 编译

```bash
cargo build --release
```

## 配置

`config.toml`：

```toml
[server]
host = "0.0.0.0"
port = 443
enable_tls = true
tls_cert = "cert.pem"
tls_key = "key.pem"
auth_timeout_secs = 10
idle_timeout_secs = 600

[[users]]
name = "admin"
token = "your_token_here"

[logging]
level = "info"
directory = "logs"
rotation = "daily"
```

## 使用

启动服务：

```bash
./target/release/ws-relay-core
```

修改配置后重新加载：

```bash
./target/release/ws-relay-core reload
```

## 客户端示例

```javascript
const ws = new WebSocket('wss://your-server:443');

ws.onopen = () => {
  // 发送认证
  ws.send(JSON.stringify({
    token: "your_token_here",
    target: "wss://target-server.com/ws"
  }));
};

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  if (msg.status === "已连接") {
    // 认证成功，开始通信
    ws.send("hello");
  }
};
```

## 工作原理

1. 客户端发送认证消息: `{"token": "...", "target": "wss://..."}`
2. ws-relay 验证 token 并连接目标服务器
3. 返回连接成功: `{"status": "已连接"}`
4. 进入透传模式，所有消息双向转发（零拷贝）

## 配置项说明

### server

- `host` - 监听地址，默认 0.0.0.0
- `port` - 监听端口，默认 443
- `enable_tls` - 是否启用 TLS，默认 true
- `tls_cert` - TLS 证书路径
- `tls_key` - TLS 私钥路径
- `auth_timeout_secs` - 认证超时秒数，默认 10
- `idle_timeout_secs` - 空闲超时秒数，默认 600（0 为禁用）
- `insecure_skip_verify` - 跳过目标服务器 TLS 验证，默认 false

### users

- `name` - 用户名，必填
- `token` - 认证 token，必填且不能重复

### logging

- `level` - 日志级别：trace, debug, info, warn, error
- `directory` - 日志目录，默认 logs
- `file_prefix` - 日志文件前缀，默认 ws-relay
- `rotation` - 轮转策略：daily, hourly, never
- `console_output` - 是否输出到控制台，默认 true

## License

MIT
