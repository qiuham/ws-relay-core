//! REST 反向代理模块

use axum::{
    body::Body,
    extract::{Path, Request},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use once_cell::sync::Lazy;
use percent_encoding::percent_decode_str;
use reqwest::Client;
use tracing::{error, info};

/// HTTP 客户端（连接池复用）
static CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .pool_max_idle_per_host(10)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .expect("Failed to create HTTP client")
});

/// REST 代理处理器
/// 路由: /rest/{target_url}
pub async fn handler(Path(target): Path<String>, req: Request) -> Response {
    // URL 解码
    let target = percent_decode_str(&target)
        .decode_utf8_lossy()
        .to_string();

    let method = req.method().clone();
    info!("REST: {} {}", method, target);

    // 提取请求头和 body（过滤掉 host，后面会自动设置）
    let headers = filter_headers(req.headers());
    let body = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            error!("读取请求体失败: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid body").into_response();
        }
    };

    // 构建并发送请求（reqwest 会自动从 URL 设置正确的 Host header）
    let resp = match CLIENT
        .request(method, &target)
        .headers(to_reqwest_headers(&headers))
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("代理请求失败: {} - {}", target, e);
            return (StatusCode::BAD_GATEWAY, format!("Proxy error: {}", e)).into_response();
        }
    };

    // 构建响应
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let body = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            error!("读取响应体失败: {}", e);
            return (StatusCode::BAD_GATEWAY, "Failed to read response").into_response();
        }
    };

    info!("REST 响应: {} -> {} ({} bytes)", target, status, body.len());

    // 返回响应（只保留安全的响应头）
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;

    // 设置 content-type
    if let Some(ct) = resp_headers.get("content-type") {
        if let Ok(v) = HeaderValue::from_bytes(ct.as_bytes()) {
            response.headers_mut().insert("content-type", v);
        }
    }

    response
}

/// 过滤掉 hop-by-hop headers、认证 header 和 host
fn filter_headers(headers: &HeaderMap) -> HeaderMap {
    const FILTERED: &[&str] = &[
        "host",        // 会从 target URL 自动设置
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailers",
        "transfer-encoding",
        "upgrade",
        "x-token",     // 移除我们的认证 header
        "accept-encoding", // 避免压缩问题
    ];

    headers
        .iter()
        .filter(|(k, _)| !FILTERED.contains(&k.as_str().to_lowercase().as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// axum HeaderMap → reqwest HeaderMap
fn to_reqwest_headers(headers: &HeaderMap) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (k, v) in headers {
        if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_str().as_bytes()) {
            if let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                map.insert(name, val);
            }
        }
    }
    map
}
