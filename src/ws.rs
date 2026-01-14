//! WebSocket 透传模块

use axum::{
    extract::{
        ws::{Message, WebSocket},
        WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message as TungMessage};
use tracing::{error, info};

/// WebSocket 处理器
/// 路由: /ws + Header X-Target-URL
pub async fn handler(ws: WebSocketUpgrade, headers: HeaderMap) -> Response {
    // 从 Header 获取 target URL
    let target = match headers.get("X-Target-URL") {
        Some(v) => match v.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return (StatusCode::BAD_REQUEST, "Invalid X-Target-URL header").into_response(),
        },
        None => return (StatusCode::BAD_REQUEST, "Missing X-Target-URL header").into_response(),
    };

    info!("WS 连接请求: {}", target);
    ws.on_upgrade(move |socket| relay(socket, target))
}

/// 双向透传
async fn relay(client_ws: WebSocket, target: String) {
    // 连接目标 WebSocket
    let target_ws = match connect_async(&target).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            error!("连接目标失败: {} - {}", target, e);
            return;
        }
    };

    info!("已连接目标: {}", target);

    let (mut client_tx, mut client_rx) = client_ws.split();
    let (mut target_tx, mut target_rx) = target_ws.split();

    // 客户端 → 目标
    let c2t = async {
        while let Some(Ok(msg)) = client_rx.next().await {
            if let Some(m) = axum_to_tungstenite(msg) {
                if target_tx.send(m).await.is_err() { break; }
            }
        }
    };

    // 目标 → 客户端
    let t2c = async {
        while let Some(Ok(msg)) = target_rx.next().await {
            if let Some(m) = tungstenite_to_axum(msg) {
                if client_tx.send(m).await.is_err() { break; }
            }
        }
    };

    // 任一方向断开则结束
    tokio::select! {
        _ = c2t => {}
        _ = t2c => {}
    }

    info!("WS 会话结束: {}", target);
}

/// axum Message → tungstenite Message
fn axum_to_tungstenite(msg: Message) -> Option<TungMessage> {
    match msg {
        Message::Text(t) => Some(TungMessage::Text(t.to_string().into())),
        Message::Binary(b) => Some(TungMessage::Binary(b.into())),
        Message::Ping(p) => Some(TungMessage::Ping(p.into())),
        Message::Pong(p) => Some(TungMessage::Pong(p.into())),
        Message::Close(_) => Some(TungMessage::Close(None)),
    }
}

/// tungstenite Message → axum Message
fn tungstenite_to_axum(msg: TungMessage) -> Option<Message> {
    match msg {
        TungMessage::Text(t) => Some(Message::Text(t.to_string().into())),
        TungMessage::Binary(b) => Some(Message::Binary(b.into())),
        TungMessage::Ping(p) => Some(Message::Ping(p.into())),
        TungMessage::Pong(p) => Some(Message::Pong(p.into())),
        TungMessage::Close(_) => Some(Message::Close(None)),
        TungMessage::Frame(_) => None,
    }
}
