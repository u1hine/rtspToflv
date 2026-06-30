//! 共享应用状态
//!
//! 包含 broadcast channel 发送端，用于 RTSP 客户端向所有 HTTP 客户端广播流消息。

use tokio::sync::broadcast;

use crate::config::Config;
use crate::rtsp::client::StreamMessage;

/// 全局共享的应用状态
///
/// 通过 axum 的 `State` 提取器注入到所有 HTTP handler 中。
#[derive(Clone)]
pub struct AppState {
    /// 流消息广播发送端
    ///
    /// 所有 HTTP-FLV 客户端通过 `subscribe()` 接收相同的流数据。
    /// 消息类型为 `StreamMessage`，包含 AVC 配置、视频帧、音频帧。
    pub tx: broadcast::Sender<StreamMessage>,

    /// 运行配置的克隆
    #[allow(dead_code)]
    pub config: Config,
}

impl AppState {
    /// 创建新的应用状态
    ///
    /// `capacity` 指定广播通道的缓冲区大小 (可容纳的消息数量)。
    /// 当消费者 (HTTP 客户端) 跟不上生产者 (RTSP 拉流) 速度时，
    /// 超出 capacity 的旧消息会被丢弃。
    pub fn new(config: Config, capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx, config }
    }

    /// 订阅流消息
    pub fn subscribe(&self) -> broadcast::Receiver<StreamMessage> {
        self.tx.subscribe()
    }
}
