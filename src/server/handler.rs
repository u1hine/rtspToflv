//! HTTP-FLV 请求处理器
//!
//! 提供 `/live/:stream_name` 端点，以 HTTP 长连接方式持续推送 FLV 流。
//! 协议: HTTP/1.1 chunked transfer encoding + Content-Type: video/x-flv
//!
//! ## 长连接架构
//!
//! 每个 HTTP 客户端连接后，handler 会 spawn 一个独立的 tokio task 作为生产者:
//!   1. 生产者 task 从 broadcast channel 订阅 RTSP 码流消息
//!   2. 生产者 task 将消息封装为 FLV Tag，通过 tokio::mpsc channel 发送
//!   3. 响应 body 从 mpsc channel 读取，axum 自动以 chunked encoding 推送
//!
//! 此架构保证了:
//! - 生产者 task 的存活周期独立于 handler 函数返回值
//! - 客户端断开连接时 mpsc receiver 被 drop，生产者 task 检测到 send 失败后退出
//! - 真正的 HTTP 长连接 (持续推送，直到客户端断开或 RTSP 源关闭)

use std::convert::Infallible;

use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};
use bytes::Bytes;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info, warn};

use crate::flv::muxer::FlvMuxer;
use crate::rtsp::client::StreamMessage;
use crate::server::state::AppState;

/// GET /live/:stream_name — HTTP-FLV 长连接流式端点
///
/// 客户端连接后持续接收 FLV 流数据，直到:
/// - 客户端断开连接 (mpsc receiver dropped → producer 退出)
/// - RTSP 源关闭 (broadcast channel 关闭 → producer 退出)
pub async fn live_handler(
    Path(stream_name): Path<String>,
    State(state): State<AppState>,
) -> Response {
    info!("HTTP-FLV 客户端连接: stream={stream_name}");

    // 订阅 RTSP 码流广播
    let mut broadcast_rx = state.subscribe();

    // 获取缓存的 AVC 解码器配置 (RTSP 可能尚未就绪，此时为 None)
    let cached_avc = state.get_avc_config().await;

    // mpsc channel: producer task → response body
    // 64 容量缓冲，平滑网络抖动
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Bytes>(64);

    // ---- 生产者 task: 广播通道 → FLV Tag → mpsc ----
    tokio::spawn(async move {
        let mut muxer = FlvMuxer::new();
        let mut avc_header_sent = false;

        // 发送 FLV Header
        if tx.send(muxer.write_header()).await.is_err() {
            return;
        }

        // 发送 PreviousTagSize0
        if tx
            .send(muxer.write_first_previous_tag_size())
            .await
            .is_err()
        {
            return;
        }

        // 发送 onMetaData
        match muxer.write_metadata() {
            Ok(meta) => {
                if tx.send(meta).await.is_err() {
                    return;
                }
            }
            Err(e) => warn!("onMetaData 生成失败: {e}"),
        }

        // 发送缓存的 AVC 配置 (RTSP 客户端已就绪的情况)
        if let Some(ref config_record) = cached_avc {
            match muxer.write_avc_sequence_header(config_record) {
                Ok(tag) => {
                    if tx.send(tag).await.is_err() {
                        return;
                    }
                    avc_header_sent = true;
                    debug!(
                        "AVC Sequence Header 已发送 (来自缓存, {} 字节)",
                        config_record.len()
                    );
                }
                Err(e) => warn!("AVC Sequence Header (缓存) 封装失败: {e}"),
            }
        }

        // 持续从广播通道读取并封装为 FLV Tag
        loop {
            match broadcast_rx.recv().await {
                Ok(msg) => {
                    let result = match msg {
                        StreamMessage::AvcConfig { config_record } => {
                            if !avc_header_sent {
                                match muxer.write_avc_sequence_header(&config_record) {
                                    Ok(tag) => {
                                        avc_header_sent = true;
                                        debug!(
                                            "AVC Sequence Header 已发送 ({} 字节)",
                                            config_record.len()
                                        );
                                        Some(tag)
                                    }
                                    Err(e) => {
                                        warn!("AVC Sequence Header 封装失败: {e}");
                                        None
                                    }
                                }
                            } else {
                                // SPS/PPS 变更，重新发送
                                muxer.write_avc_sequence_header(&config_record).ok()
                            }
                        }
                        StreamMessage::Video {
                            data,
                            timestamp_ms,
                            is_keyframe,
                        } => {
                            if !avc_header_sent {
                                debug!("跳过视频帧 (等待 AVC 配置)");
                                continue;
                            }
                            match muxer.write_video_tag(&data, timestamp_ms, is_keyframe) {
                                Ok(tag) => Some(tag),
                                Err(e) => {
                                    warn!("视频 Tag 封装失败: {e}");
                                    None
                                }
                            }
                        }
                        StreamMessage::Audio { data, timestamp_ms } => {
                            if !avc_header_sent {
                                continue;
                            }
                            match muxer.write_audio_tag(&data, timestamp_ms) {
                                Ok(tag) => Some(tag),
                                Err(e) => {
                                    warn!("音频 Tag 封装失败: {e}");
                                    None
                                }
                            }
                        }
                    };

                    if let Some(tag) = result {
                        // send 失败 = 客户端断开 → mpsc receiver dropped → 退出 task
                        if tx.send(tag).await.is_err() {
                            debug!("HTTP 客户端断开, 生产者 task 退出");
                            return;
                        }
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    warn!("HTTP 客户端落后，跳过 {skipped} 条消息");
                    // 不重置 avc_header_sent — 客户端已收到过 AVC 配置,
                    // 且 FLV 播放器可以容忍中间丢帧。
                    continue;
                }
                Err(RecvError::Closed) => {
                    info!("广播通道已关闭，生产者 task 退出");
                    return;
                }
            }
        }
    });

    // ---- 响应 body: 从 mpsc channel 读取 ----
    let stream = async_stream::stream! {
        while let Some(bytes) = rx.recv().await {
            yield Ok::<Bytes, Infallible>(bytes);
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "video/x-flv")
        .header("Cache-Control", "no-cache, no-store, must-revalidate")
        .header("Pragma", "no-cache")
        .header("Expires", "0")
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "GET, OPTIONS")
        .header("Connection", "keep-alive")
        .header("Transfer-Encoding", "chunked")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|e| {
            warn!("构建 HTTP 响应失败: {e}");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal Server Error"))
                .unwrap()
        })
}
