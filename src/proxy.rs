//! 双向透传核心（forward 优化版）

use anyhow::Result;
use futures_util::StreamExt;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream};
use tokio::net::TcpStream;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{timeout, Duration};
use tracing::info;

/// 双向透传（使用 forward 高效转发）
pub async fn run_proxy<S>(
    client_ws: WebSocketStream<S>,
    target_ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    idle_timeout_secs: u64,  // 0 = 禁用空闲超时
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (client_sink, client_stream) = client_ws.split();
    let (target_sink, target_stream) = target_ws.split();

    // 双向转发任务
    let client_to_target = client_stream.forward(target_sink);
    let target_to_client = target_stream.forward(client_sink);

    // 根据是否启用超时选择不同的执行策略
    let result = if idle_timeout_secs > 0 {
        // 启用超时：使用 tokio::time::timeout
        let duration = Duration::from_secs(idle_timeout_secs);
        let transfer = async {
            tokio::try_join!(client_to_target, target_to_client)
        };

        match timeout(duration, transfer).await {
            Ok(Ok(_)) => {
                info!("连接正常关闭");
                Ok(())
            }
            Ok(Err(e)) => {
                info!("连接错误: {}", e);
                Err(anyhow::anyhow!("转发错误: {}", e))
            }
            Err(_) => {
                info!("连接空闲超时");
                Ok(())
            }
        }
    } else {
        // 不启用超时：直接执行
        match tokio::try_join!(client_to_target, target_to_client) {
            Ok(_) => {
                info!("连接正常关闭");
                Ok(())
            }
            Err(e) => {
                info!("连接错误: {}", e);
                Err(anyhow::anyhow!("转发错误: {}", e))
            }
        }
    };

    result
}
