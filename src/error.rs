//! 统一错误类型定义
//!
//! 使用 `thiserror` 定义库级错误类型，涵盖 RTSP 拉流、FLV 封装、HTTP 服务三大模块。
//! 所有错误均实现 `Into<anyhow::Error>`，`main.rs` 中使用 `anyhow` 做顶层错误报告。

use thiserror::Error;

/// 网关服务的统一错误类型
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum AppError {
    /// RTSP 连接、认证、流读取相关错误
    #[error("RTSP 错误: {0}")]
    Rtsp(String),

    /// FLV 封装失败、无效 NAL 单元等
    #[error("FLV 封装错误: {0}")]
    Flv(String),

    /// HTTP 服务启动、请求处理相关错误
    #[error("HTTP 服务错误: {0}")]
    Server(String),

    /// 广播通道发送失败 (所有接收端已断开)
    #[error("广播通道已关闭")]
    BroadcastClosed,
}

#[allow(dead_code)]
impl AppError {
    /// 创建一个 RTSP 错误
    pub fn rtsp(msg: impl Into<String>) -> Self {
        Self::Rtsp(msg.into())
    }

    /// 创建一个 FLV 封装错误
    pub fn flv(msg: impl Into<String>) -> Self {
        Self::Flv(msg.into())
    }

    /// 创建一个 HTTP 服务错误
    pub fn server(msg: impl Into<String>) -> Self {
        Self::Server(msg.into())
    }
}
