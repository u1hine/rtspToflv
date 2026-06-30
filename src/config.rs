//! 命令行参数与运行配置
//!
//! 使用 `clap` 的 derive 模式解析命令行参数，支持：
//! - RTSP 源地址 (必填)
//! - HTTP 服务监听端口 (默认 8080)
//! - 日志等级 (默认 info)
//! - 重连间隔 (默认 3 秒)
//! - 流名称 (默认 "stream")

use clap::Parser;

/// RTSP 转 HTTP-FLV 网关服务
///
/// 从 IPC 摄像头拉取 RTSP 码流，转封装为 HTTP-FLV 格式，
/// 提供给 Web 端 (微信小程序 live-player) 播放。
#[derive(Parser, Debug, Clone)]
#[command(name = "rtsp-to-flv", version, about)]
pub struct Config {
    /// RTSP 摄像头源地址
    ///
    /// 示例: rtsp://admin:password@192.168.1.64:554/Streaming/Channels/101
    #[arg(short = 'u', long, env = "RTSP_URL")]
    pub rtsp_url: String,

    /// HTTP 服务监听端口
    #[arg(short = 'p', long, default_value = "8080", env = "HTTP_PORT")]
    pub port: u16,

    /// 日志等级 (trace, debug, info, warn, error)
    #[arg(short = 'l', long, default_value = "info", env = "LOG_LEVEL")]
    pub log_level: String,

    /// RTSP 断连重试间隔 (秒)
    #[arg(short = 'r', long, default_value = "3", env = "RECONNECT_INTERVAL")]
    pub reconnect_interval: u64,

    /// 流名称 (访问路径: /live/{stream_name})
    #[arg(short = 'n', long, default_value = "stream", env = "STREAM_NAME")]
    pub stream_name: String,

    /// 最大重连退避时间 (秒, 默认 30)
    #[arg(long, default_value = "30", env = "MAX_BACKOFF")]
    pub max_backoff: u64,
}

impl Config {
    /// 获取监听地址字符串
    pub fn listen_addr(&self) -> String {
        format!("0.0.0.0:{}", self.port)
    }

    /// 获取 HTTP-FLV 播放地址
    pub fn http_flv_url(&self) -> String {
        format!("http://127.0.0.1:{}/live/{}", self.port, self.stream_name)
    }
}
