//! 共享应用状态
//!
//! 包含 broadcast channel 发送端，用于 RTSP 客户端向所有 HTTP 客户端广播流消息。

use bytes::Bytes;
use tokio::sync::{RwLock, broadcast};

use crate::config::Config;
use crate::rtsp::client::StreamMessage;

/// 全局共享的应用状态
///
/// 通过 axum 的 `State` 提取器注入到所有 HTTP handler 中。
#[derive(Clone)]
pub struct AppState {
    /// 流消息广播发送端
    pub tx: broadcast::Sender<StreamMessage>,

    /// 缓存的 AVC 解码器配置记录 (SPS/PPS)
    ///
    /// 新 HTTP 客户端连接时直接读取，无需等待 RTSP 客户端重新发送。
    /// 首次连接之前此值为 None。
    avc_config: std::sync::Arc<RwLock<Option<Bytes>>>,

    /// 运行配置
    #[allow(dead_code)]
    pub config: Config,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(config: Config, capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            avc_config: std::sync::Arc::new(RwLock::new(None)),
            config,
        }
    }

    /// 订阅流消息
    pub fn subscribe(&self) -> broadcast::Receiver<StreamMessage> {
        self.tx.subscribe()
    }

    /// 更新缓存的 AVC 配置
    pub async fn update_avc_config(&self, config: Bytes) {
        *self.avc_config.write().await = Some(config);
    }

    /// 获取缓存的 AVC 配置 (可能为 None)
    pub async fn get_avc_config(&self) -> Option<Bytes> {
        self.avc_config.read().await.clone()
    }
}
