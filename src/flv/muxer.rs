//! FLV 封装器 (Muxer)
//!
//! 将 H.264 视频帧和 AAC 音频帧封装为标准 FLV 格式的 Tag。
//! FLV 流结构:
//!   1. FLV Header (9 字节)
//!   2. PreviousTagSize0 (4 字节, 值为 0)
//!   3. Script Tag (onMetaData) — 流的元数据，播放器初始化必需
//!   4. AVC Sequence Header — H.264 解码器配置 (SPS/PPS)
//!   5. 循环发送 Video/Audio Tags
//!
//! 参考: Adobe Flash Video File Format Specification v10.1

use bytes::Bytes;
use oxideav_flv as flv;

use crate::error::AppError;

/// FLV 封装器
///
/// 每个 HTTP-FLV 客户端持有独立的 FlvMuxer 实例，
/// 负责生成 FLV Header、onMetaData 以及封装音视频 Tag。
pub struct FlvMuxer {
    /// 视频宽度 (来自 SPS 解析或默认值)
    width: u16,
    /// 视频高度
    height: u16,
    /// 是否已写入 FLV Header
    header_sent: bool,
}

impl FlvMuxer {
    /// 创建新的 FLV 封装器
    pub fn new() -> Self {
        Self {
            width: 1920,
            height: 1080,
            header_sent: false,
        }
    }

    /// 设置视频分辨率
    #[allow(dead_code)]
    pub fn set_resolution(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// 生成 FLV 文件头 (9 字节)
    ///
    /// 格式: "FLV" (3B) + 版本号 1 (1B) + 类型标志 (1B, bit2=音频, bit0=视频) + 头长度 9 (4B)
    pub fn write_header(&mut self) -> Bytes {
        self.header_sent = true;
        let mut buf = Vec::with_capacity(9);
        // has_audio=true, has_video=true
        flv::header::write(&mut buf, true, true).expect("写入 FLV Header 失败");
        Bytes::from(buf)
    }

    /// 生成 PreviousTagSize0 字段 (4 字节, 值为 0)
    ///
    /// FLV 规范要求在 Header 之后第一个 Tag 之前写入此字段。
    pub fn write_first_previous_tag_size(&self) -> Bytes {
        let mut buf = Vec::with_capacity(4);
        flv::tag::write_first_previous_tag_size(&mut buf).expect("写入 PreviousTagSize0 失败");
        Bytes::from(buf)
    }

    /// 生成 onMetaData 脚本 Tag
    ///
    /// 包含流的元数据：时长、分辨率、帧率、编码器信息等。
    pub fn write_metadata(&self) -> Result<Bytes, AppError> {
        let bag = flv::script::MetadataBag::new()
            .number("duration", 0.0)
            .number("width", self.width as f64)
            .number("height", self.height as f64)
            .number("videodatarate", 0.0)
            .number("framerate", 25.0)
            .number("videocodecid", 7.0) // AVC = 7
            .number("audiodatarate", 0.0)
            .number("audiosamplerate", 44100.0)
            .number("audiosamplesize", 16.0)
            .boolean("stereo", true)
            .number("audiocodecid", 10.0) // AAC = 10
            .string("encoder", "rtsp-to-flv")
            .number("filesize", 0.0);

        let mut buf = Vec::new();
        flv::script::write_on_metadata(&mut buf, &bag)
            .map_err(|e| AppError::flv(format!("写入 onMetaData 失败: {e}")))?;

        Ok(Bytes::from(buf))
    }

    /// 生成 AVC Sequence Header Tag
    ///
    /// `config_record` 是 AVCDecoderConfigurationRecord (ISO/IEC 14496-15) 的完整字节序列，
    /// 包含 SPS 和 PPS 参数集。客户端接收到此 Tag 后才能初始化解码器。
    ///
    /// retina 通过 `VideoParameters::extra_data()` 提供此数据。
    pub fn write_avc_sequence_header(&mut self, config_record: &[u8]) -> Result<Bytes, AppError> {
        let mut buf = Vec::new();
        flv::tag::write_avc_sequence_header(&mut buf, 0, config_record)
            .map_err(|e| AppError::flv(format!("写入 AVC Sequence Header 失败: {e}")))?;
        Ok(Bytes::from(buf))
    }

    /// 生成 H.264 视频 Tag
    ///
    /// 将单个视频帧的 NAL 单元数据封装为 FLV Video Tag。
    ///
    /// # 参数
    /// - `access_unit`: 长度前缀格式的 NAL 单元序列 (retina 的 VideoFrame::data() 输出)
    /// - `timestamp_ms`: 帧的展示时间戳 (毫秒)
    /// - `is_keyframe`: 是否为关键帧 (IDR)
    pub fn write_video_tag(
        &mut self,
        access_unit: &[u8],
        timestamp_ms: u32,
        is_keyframe: bool,
    ) -> Result<Bytes, AppError> {
        let mut buf = Vec::new();
        // composition_time = 0 (IPC 摄像头通常 PTS == DTS)
        flv::tag::write_avc_nalu_tag(
            &mut buf,
            timestamp_ms,
            is_keyframe,
            0, // composition_time_ms
            access_unit,
        )
        .map_err(|e| AppError::flv(format!("写入视频 Tag 失败: {e}")))?;
        Ok(Bytes::from(buf))
    }

    /// 生成 AAC 音频 Tag
    ///
    /// 将 AAC raw access unit 封装为 FLV Audio Tag。
    ///
    /// # 参数
    /// - `raw_au`: AAC 原始访问单元 (retina 的 AudioFrame::data() 输出)
    /// - `timestamp_ms`: 帧的展示时间戳 (毫秒)
    pub fn write_audio_tag(&mut self, raw_au: &[u8], timestamp_ms: u32) -> Result<Bytes, AppError> {
        let mut buf = Vec::new();
        flv::tag::write_aac_raw_tag(&mut buf, timestamp_ms, raw_au)
            .map_err(|e| AppError::flv(format!("写入音频 Tag 失败: {e}")))?;
        Ok(Bytes::from(buf))
    }

    /// 是否已发送 FLV Header
    #[allow(dead_code)]
    pub fn has_header(&self) -> bool {
        self.header_sent
    }
}

impl Default for FlvMuxer {
    fn default() -> Self {
        Self::new()
    }
}

// ========== NAL 单元校验辅助函数 ==========

/// 校验 H.264 NAL 单元是否有效
///
/// 检查 forbidden_zero_bit (最高位必须为 0)。
#[allow(dead_code)]
pub fn validate_nalu(nalu: &[u8]) -> bool {
    if nalu.is_empty() {
        return false;
    }
    // forbidden_zero_bit (最高位) 必须为 0
    if nalu[0] & 0x80 != 0 {
        return false;
    }
    // nal_unit_type 合法范围: 1-31 (0 未定义)
    let nal_type = nalu[0] & 0x1F;
    nal_type != 0
}

/// 校验 AAC 帧的 ADTS 头
///
/// ADTS 固定头: sync word (12 bit, 必须为 0xFFF)
#[allow(dead_code)]
pub fn validate_aac_frame(data: &[u8]) -> bool {
    if data.len() < 7 {
        return false;
    }
    // ADTS sync word: 前 12 位必须全为 1 (0xFFF)
    (data[0] == 0xFF) && ((data[1] & 0xF0) == 0xF0)
}
