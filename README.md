# ws-relay-core

高性能 WebSocket + REST 中继代理，用于交易所 API 转发。

## 特性

- **同端口双协议** - WS 和 REST 共用 443 端口，路由分发
- **零配置转发** - 目标 URL 由客户端动态指定
- **Token 认证** - 支持 Query 参数和 Header 两种方式
- **TLS 加密** - 基于 rustls，安全高效
- **连接池复用** - REST 请求复用 HTTP 连接

## 架构

```
客户端 ──→ ws-relay-core ──→ 目标服务器
          /ws/*   → WebSocket 双向透传
          /rest/* → HTTP 反向代理
```

## 编译

```bash
cargo build --release
```

## 配置

`config.toml`:

```toml
[server]
host = "0.0.0.0"
port = 443
tls_cert = "cert.pem"
tls_key = "key.pem"

[[users]]
name = "admin"
token = "your_token_here"
```

生成自签名证书：

```bash
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes -subj '/CN=localhost'
```

## 启动

```bash
./target/release/ws-relay-core config.toml
```

## 使用

### WebSocket

URL 格式: `wss://relay:443/ws/<target_url>?token=xxx`

```python
import websockets
import urllib.parse

target = "wss://ws.okx.com:8443/ws/v5/public"
url = f"wss://relay:443/ws/{urllib.parse.quote(target, safe='')}?token=xxx"

async with websockets.connect(url, ssl=ssl_ctx) as ws:
    await ws.send('{"op":"subscribe","args":[{"channel":"tickers","instId":"BTC-USDT"}]}')
    async for msg in ws:
        print(msg)
```

### REST

URL 格式: `https://relay:443/rest/<target_url>`
认证方式: Header `X-Token: xxx`

```python
import requests

target = "https://api.binance.com/api/v3/ticker/price?symbol=BTCUSDT"
url = f"https://relay:443/rest/{urllib.parse.quote(target, safe='')}"

resp = requests.get(url, headers={"X-Token": "xxx"}, verify=False)
print(resp.json())
```

## 性能

| 指标 | 数值 |
|------|------|
| WS 延迟开销 | +0.5ms |
| REST 延迟开销 | +1ms |
| WS 吞吐量 | 45,000 msg/s |

## 依赖

- Rust 1.70+
- tokio (异步运行时)
- axum 0.8 (Web 框架)
- tokio-tungstenite (WebSocket)
- reqwest (HTTP 客户端)
- rustls (TLS)

## License

MIT
