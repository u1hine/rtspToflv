//! HTTP 服务模块
//!
//! 基于 `axum` 提供 HTTP-FLV 流式接口，
//! 支持多个客户端同时拉取同一路流 (通过 tokio::broadcast 多路复用)。

pub mod handler;
pub mod state;
