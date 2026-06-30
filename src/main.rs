//! RTSP → HTTP-FLV 网关服务
//!
//! 从 RTSP 摄像头拉取 H.264/AAC 码流，转封装为 FLV 格式，
//! 通过 HTTP 长连接推送给 Web 播放器 (微信小程序 live-player 等)。
//!
//! ## 架构
//!
//! ```text
//! IPC Camera (RTSP) → retina (RTP depacketize) → H.264 NALs / AAC frames
//!     → broadcast channel → FlvMuxer (FLV封装) → axum (HTTP-FLV) → 浏览器/小程序
//! ```
//!
//! ## 使用示例
//!
//! ```bash
//! cargo run -- -u rtsp://admin:pass@192.168.1.64:554/Streaming/Channels/101
//! cargo run -- -u rtsp://... -p 8080 -n camera1
//! ```
//!
//! 播放地址: `http://127.0.0.1:8080/live/stream`
//!
//! ## 环境变量
//!
//! - `RTSP_URL` — RTSP 源地址
//! - `HTTP_PORT` — HTTP 服务端口
//! - `LOG_LEVEL` — 日志等级 (trace/debug/info/warn/error)
//! - `RECONNECT_INTERVAL` — 重连间隔 (秒)
//! - `STREAM_NAME` — 流名称
//! - `MAX_BACKOFF` — 最大退避时间 (秒)

mod config;
mod error;
mod flv;
mod rtsp;
mod server;

use std::net::SocketAddr;

use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::rtsp::client::RtspClient;
use crate::server::handler::live_handler;
use crate::server::state::AppState;

/// 广播通道容量: 64 条消息
///
/// 在 25fps 下约 2.5 秒缓冲，足够容忍网络抖动。
/// 注: 每条消息可能包含多个 FLV Tag。
const BROADCAST_CAPACITY: usize = 64;

#[tokio::main]
async fn main() {
    // ---- 解析命令行参数 ----
    let config = Config::parse();

    // ---- 初始化日志系统 ----
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.log_level)),
        )
        .with_target(false)
        .init();

    info!("rtsp-to-flv 启动");
    info!("RTSP 源: {}", config.rtsp_url);
    info!("监听地址: {}", config.listen_addr());
    info!("播放地址: {}", config.http_flv_url());

    // ---- 创建共享状态 ----
    let state = AppState::new(config.clone(), BROADCAST_CAPACITY);

    // ---- 启动 RTSP 拉流任务 ----
    let rtsp_client = RtspClient::new(config.clone(), state.tx.clone());
    let rtsp_handle = tokio::spawn(async move {
        rtsp_client.run().await;
    });

    // ---- 构建路由 ----
    let app = axum::Router::new()
        .route("/live/{stream_name}", axum::routing::get(live_handler))
        .with_state(state);

    // ---- 绑定端口 ----
    let addr: SocketAddr = config.listen_addr().parse().expect("无效的监听地址");

    info!(
        "HTTP-FLV 服务已启动: http://{addr}/live/{}",
        config.stream_name
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            error!("绑定端口 {} 失败: {e}", config.port);
            std::process::exit(1);
        });

    // ---- 启动 HTTP 服务 (带优雅关闭) ----
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            error!("HTTP 服务错误: {e}");
        });

    // 停止 RTSP 任务
    rtsp_handle.abort();
    let _ = rtsp_handle.await;
    info!("rtsp-to-flv 已关闭");
}

/// 等待系统关闭信号 (Ctrl+C)
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("无法注册 Ctrl+C 信号处理");
    info!("收到关闭信号，正在停止服务...");
}
