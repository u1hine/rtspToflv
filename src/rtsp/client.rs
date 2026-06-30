//! RTSP 客户端封装
//!
//! 基于 `retina` 库从 RTSP 摄像头拉取 H.264/AAC 码流。
//! 功能:
//! - TCP 交错传输 (穿透防火墙/NAT)
//! - 自动重连 + 指数退避
//! - 码流数据校验
//! - 通过 broadcast channel 分发数据给所有 HTTP-FLV 客户端

use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use retina::client::{PlayOptions, Session, SessionOptions, SetupOptions, Transport};
use retina::codec::{CodecItem, ParametersRef};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use url::Url;

use crate::config::Config;
use crate::error::AppError;

/// 内部消息类型 —— 通过 broadcast channel 传递
#[derive(Debug, Clone)]
pub enum StreamMessage {
    /// AVC 解码器配置记录 (SPS + PPS)
    /// 新客户端连接后需要首先接收此消息以初始化解码器
    AvcConfig {
        /// AVCDecoderConfigurationRecord 的完整字节序列
        config_record: Bytes,
    },
    /// 视频帧 (H.264 NAL 单元, AVCC 长度前缀格式)
    Video {
        /// 长度前缀格式的 NAL 单元序列
        data: Bytes,
        /// 展示时间戳 (毫秒)
        timestamp_ms: u32,
        /// 是否为关键帧 (IDR)
        is_keyframe: bool,
    },
    /// 音频帧 (AAC raw access unit)
    Audio {
        /// AAC 原始访问单元
        data: Bytes,
        /// 展示时间戳 (毫秒)
        timestamp_ms: u32,
    },
}

/// RTSP 拉流客户端
///
/// 管理与摄像头的 RTSP 连接，将持续拉取到的视频/音频帧
/// 通过 broadcast channel 分发给所有 HTTP-FLV 客户端。
pub struct RtspClient {
    /// 运行配置
    config: Config,
    /// 流消息广播发送端
    tx: broadcast::Sender<StreamMessage>,
    /// 当前退避时间 (秒)，成功连接后重置为 1
    backoff_secs: u64,
}

impl RtspClient {
    /// 创建新的 RTSP 客户端
    pub fn new(config: Config, tx: broadcast::Sender<StreamMessage>) -> Self {
        Self {
            config,
            tx,
            backoff_secs: 1,
        }
    }

    /// 启动 RTSP 拉流主循环
    ///
    /// 包含完整的 连接→拉流→断开→重连 生命周期管理。
    /// 此函数会一直运行，直到程序关闭 (broadcast channel 关闭)。
    pub async fn run(mut self) {
        info!("RTSP 客户端启动，源地址: {}", self.config.rtsp_url);

        loop {
            match self.connect_and_stream().await {
                Ok(()) => {
                    warn!("RTSP 码流正常结束，准备重连...");
                }
                Err(e) => {
                    error!("RTSP 连接/流错误: {e}，准备重连...");
                }
            }

            // 检查广播通道是否已关闭 (所有接收端断开)
            if self.tx.receiver_count() == 0 {
                info!("所有 HTTP 客户端已断开，停止拉流");
                break;
            }

            // 等待退避时间后重连
            let wait = std::cmp::max(self.config.reconnect_interval, self.backoff_secs);
            info!("{} 秒后重连 RTSP...", wait);
            tokio::time::sleep(Duration::from_secs(wait)).await;

            // 指数退避: 翻倍，上限 max_backoff 秒
            self.backoff_secs =
                std::cmp::min(self.backoff_secs.saturating_mul(2), self.config.max_backoff);
        }
    }

    /// 连接摄像头并持续拉流
    ///
    /// 返回 Ok(()) 表示流自然结束，Err 表示连接或读取错误。
    async fn connect_and_stream(&mut self) -> Result<(), AppError> {
        // ---- 第 1 步: 解析 RTSP URL ----
        let url = Url::parse(&self.config.rtsp_url)
            .map_err(|e| AppError::rtsp(format!("无效的 RTSP URL: {e}")))?;

        // ---- 第 2 步: DESCRIBE (获取媒体描述) ----
        info!("正在连接 RTSP: {}", self.config.rtsp_url);

        let session = Session::describe(url, SessionOptions::default())
            .await
            .map_err(|e| AppError::rtsp(format!("RTSP DESCRIBE 失败: {e}")))?;

        let num_streams = session.streams().len();
        info!("RTSP 流信息: {} 个媒体流", num_streams);
        for (i, stream) in session.streams().iter().enumerate() {
            info!(
                "  流 {}: media={}, encoding={}",
                i,
                stream.media(),
                stream.encoding_name()
            );
        }

        // ---- 第 3 步: SETUP (为每个流建立传输通道) ----
        let mut session = session;
        for i in 0..num_streams {
            let setup_opts = SetupOptions::default().transport(Transport::default());
            session
                .setup(i, setup_opts)
                .await
                .map_err(|e| AppError::rtsp(format!("RTSP SETUP 流 {i} 失败: {e}")))?;
            debug!("RTSP SETUP 流 {i} 成功");
        }

        // ---- 第 4 步: PLAY (开始播放) ----
        let playing = session
            .play(PlayOptions::default())
            .await
            .map_err(|e| AppError::rtsp(format!("RTSP PLAY 失败: {e}")))?;

        // ---- 第 5 步: 转换为 Demuxed 模式 (自动解包) ----
        let mut demuxed = playing
            .demuxed()
            .map_err(|e| AppError::rtsp(format!("Demuxed 初始化失败: {e}")))?;

        info!("RTSP 连接成功，开始拉流");
        self.backoff_secs = 1; // 重置退避

        let mut _avc_config_sent = false;

        // ---- 第 6 步: 持续读取解包后的帧 ----
        while let Some(frame_result) = demuxed.next().await {
            match frame_result {
                Ok(CodecItem::VideoFrame(video)) => {
                    // 检查是否有新的解码器参数 (SPS/PPS 更新)
                    if video.has_new_parameters() {
                        // 获取视频流的参数和 extra_data
                        let config_record =
                            get_video_extra_data(demuxed.streams(), video.stream_id());
                        if let Some(ref cr) = config_record {
                            debug!(
                                "新的 AVC 配置记录: stream={}, len={}",
                                video.stream_id(),
                                cr.len()
                            );

                            // 广播 AVC 配置给所有客户端
                            let msg = StreamMessage::AvcConfig {
                                config_record: cr.clone(),
                            };
                            if self.tx.send(msg).is_err() {
                                info!("广播通道已关闭，停止拉流");
                                return Ok(());
                            }
                            _avc_config_sent = true;
                        }
                    }

                    let data = video.data();
                    if !data.is_empty() {
                        let timestamp = video.timestamp();
                        let ts_ms = (timestamp.elapsed_secs() * 1000.0) as u32;
                        let is_keyframe = video.is_random_access_point();

                        debug!(
                            "视频帧: size={}, keyframe={}, ts={}ms",
                            data.len(),
                            is_keyframe,
                            ts_ms
                        );

                        let msg = StreamMessage::Video {
                            data: Bytes::copy_from_slice(data),
                            timestamp_ms: ts_ms,
                            is_keyframe,
                        };
                        if self.tx.send(msg).is_err() {
                            info!("广播通道已关闭，停止拉流");
                            return Ok(());
                        }
                    }
                }
                Ok(CodecItem::AudioFrame(audio)) => {
                    let data = audio.data();
                    if !data.is_empty() {
                        let timestamp = audio.timestamp();
                        let ts_ms = (timestamp.elapsed_secs() * 1000.0) as u32;

                        debug!("音频帧: size={}, ts={}ms", data.len(), ts_ms);

                        let msg = StreamMessage::Audio {
                            data: Bytes::copy_from_slice(data),
                            timestamp_ms: ts_ms,
                        };
                        if self.tx.send(msg).is_err() {
                            info!("广播通道已关闭，停止拉流");
                            return Ok(());
                        }
                    }
                }
                Ok(CodecItem::Rtcp(_)) => {
                    // RTCP 包 -- 静默忽略
                }
                Ok(CodecItem::MessageFrame(_)) => {
                    // ONVIF 元数据 -- 静默忽略
                }
                Ok(_other) => {
                    // 未知/未来扩展的帧类型 -- 静默忽略
                }
                Err(e) => {
                    warn!("RTSP 帧解码错误: {e}，跳过此帧继续");
                    continue;
                }
            }
        }

        info!("RTSP 流结束");
        Ok(())
    }
}

/// 从指定视频流的参数中提取 extra_data (AVCDecoderConfigurationRecord)
///
/// retina 的 `VideoParameters::extra_data()` 对 H.264 返回的就是
/// `AVCDecoderConfigurationRecord` 字节序列，可以直接传给 FLV muxer。
fn get_video_extra_data(streams: &[retina::client::Stream], stream_id: usize) -> Option<Bytes> {
    let stream = streams.get(stream_id)?;
    let params = stream.parameters()?;

    // 提取视频参数的 extra_data
    match params {
        ParametersRef::Video(vp) => {
            let extra = vp.extra_data();
            if extra.is_empty() {
                None
            } else {
                Some(Bytes::copy_from_slice(extra))
            }
        }
        _ => None,
    }
}
