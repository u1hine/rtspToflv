//! HTTP-FLV 请求处理器
//!
//! 提供 `/live/:stream_name` 端点，以 HTTP 长连接方式持续推送 FLV 流。
//! 协议: HTTP chunked transfer encoding + Content-Type: video/x-flv
//!
//! FLV 流发送顺序 (每个新客户端):
//!   1. FLV Header (9 字节)
//!   2. PreviousTagSize0 (4 字节)
//!   3. onMetaData Script Tag
//!   4. AVC Sequence Header (等待 RTSP 客户端发送解码器配置)
//!   5. 循环发送 Video/Audio Tags

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

/// GET /live/:stream_name — HTTP-FLV 流式端点
///
/// 客户端连接后持续接收 FLV 流数据，直到:
/// - 客户端断开连接
/// - RTSP 源关闭
/// - 广播通道关闭
pub async fn live_handler(
    Path(stream_name): Path<String>,
    State(state): State<AppState>,
) -> Response {
    info!("HTTP-FLV 客户端连接: stream={stream_name}");

    let mut rx = state.subscribe();

    // 如果 RTSP 客户端已就绪，获取缓存的 AVC 配置
    let cached_avc = state.get_avc_config().await;

    let stream = async_stream::stream! {
        let mut muxer = FlvMuxer::new();
        let mut avc_header_sent = false;

        // 第 1 步: 发送 FLV Header
        yield Ok::<Bytes, Infallible>(muxer.write_header());

        // 第 2 步: 发送 PreviousTagSize0
        yield Ok(muxer.write_first_previous_tag_size());

        // 第 3 步: 发送 onMetaData Script Tag
        match muxer.write_metadata() {
            Ok(meta) => yield Ok(meta),
            Err(e) => {
                warn!("onMetaData 生成失败: {e}");
            }
        }

        // 第 3.5 步: 如果缓存中有 AVC 配置，直接发送 (避免等待 RTSP 客户端重新广播)
        if let Some(ref config_record) = cached_avc {
            match muxer.write_avc_sequence_header(config_record) {
                Ok(tag) => {
                    yield Ok(tag);
                    avc_header_sent = true;
                    debug!("AVC Sequence Header 已发送 (来自缓存, {} 字节)", config_record.len());
                }
                Err(e) => {
                    warn!("AVC Sequence Header (缓存) 封装失败: {e}");
                }
            }
        }

        // 第 4+ 步: 持续接收并转发 FLV Tag
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    match msg {
                        StreamMessage::AvcConfig { config_record } => {
                            // 生成 AVC Sequence Header Tag
                            if !avc_header_sent {
                                match muxer.write_avc_sequence_header(&config_record) {
                                    Ok(tag) => {
                                        yield Ok(tag);
                                        avc_header_sent = true;
                                        debug!("AVC Sequence Header 已发送 ({} 字节)", config_record.len());
                                    }
                                    Err(e) => {
                                        warn!("AVC Sequence Header 封装失败: {e}");
                                    }
                                }
                            } else {
                                // 解码器配置更新 — 重新发送 (用于 SPS/PPS 变更)
                                match muxer.write_avc_sequence_header(&config_record) {
                                    Ok(tag) => {
                                        yield Ok(tag);
                                        debug!("AVC Sequence Header 已更新");
                                    }
                                    Err(e) => {
                                        warn!("AVC Sequence Header 更新失败: {e}");
                                    }
                                }
                            }
                        }
                        StreamMessage::Video { data, timestamp_ms, is_keyframe } => {
                            // 等待 AVC 配置到达后再发送视频数据
                            // (播放器需要先收到解码器配置才能解码)
                            if !avc_header_sent {
                                debug!("跳过视频帧 (等待 AVC 配置)");
                                continue;
                            }
                            match muxer.write_video_tag(&data, timestamp_ms, is_keyframe) {
                                Ok(tag) => yield Ok(tag),
                                Err(e) => warn!("视频 Tag 封装失败: {e}"),
                            }
                        }
                        StreamMessage::Audio { data, timestamp_ms } => {
                            if !avc_header_sent {
                                continue;
                            }
                            match muxer.write_audio_tag(&data, timestamp_ms) {
                                Ok(tag) => yield Ok(tag),
                                Err(e) => warn!("音频 Tag 封装失败: {e}"),
                            }
                        }
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    warn!("HTTP 客户端落后，跳过 {skipped} 条消息");
                    // FLV 播放器可以容忍丢帧，继续读取最新数据。
                    // 注意: 不重置 avc_header_sent，因为 AVC 配置已在 lag 之前接收，
                    // 且 RTSP 客户端仅在 SPS/PPS 变更时才重新发送 (极少发生)。
                    // 重置会导致后续所有音视频帧被永久跳过。
                    continue;
                }
                Err(RecvError::Closed) => {
                    info!("广播通道已关闭，结束 HTTP-FLV 流");
                    break;
                }
            }
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
        .body(Body::from_stream(stream))
        .unwrap_or_else(|e| {
            warn!("构建 HTTP 响应失败: {e}");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal Server Error"))
                .unwrap()
        })
}
