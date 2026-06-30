//! RTSP 拉流模块
//!
//! 使用 `retina` 库从 RTSP 摄像头拉取 H.264/AAC 码流，
//! 包含自动重连、指数退避等容错机制。

pub mod client;
