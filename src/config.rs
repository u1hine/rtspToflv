//! 命令行参数与运行配置
//!
//! 使用 `clap` 的 derive 模式解析命令行参数。

use clap::Parser;

/// RTSP → HTTP-FLV 网关服务
///
/// 从 IPC 摄像头拉取 RTSP 码流，转封装为 HTTP-FLV 格式，
/// 提供给 Web 端 (微信小程序 live-player) 播放。
///
/// 示例:
///   rtsp-to-flv -u rtsp://admin:pass@192.168.1.64:554/Streaming/Channels/101
///   rtsp-to-flv -u rtsp://... -q --log-file /var/log/rtsp-to-flv.log
#[derive(Parser, Debug, Clone)]
#[command(name = "rtsp-to-flv", version, about)]
pub struct Config {
    /// RTSP 摄像头源地址
    #[arg(short = 'u', long, env = "RTSP_URL")]
    pub rtsp_url: String,

    /// HTTP 服务监听端口
    #[arg(short = 'p', long, default_value = "8080", env = "HTTP_PORT")]
    pub port: u16,

    /// 日志等级 (off, error, warn, info, debug, trace)
    #[arg(short = 'l', long, default_value = "info", env = "LOG_LEVEL")]
    pub log_level: String,

    /// RTSP 断连重试间隔 (秒)
    #[arg(short = 'r', long, default_value = "3", env = "RECONNECT_INTERVAL")]
    pub reconnect_interval: u64,

    /// 流名称 (访问路径: /live/{stream_name})
    #[arg(short = 'n', long, default_value = "stream", env = "STREAM_NAME")]
    pub stream_name: String,

    /// 最大重连退避时间 (秒)
    #[arg(long, default_value = "30", env = "MAX_BACKOFF")]
    pub max_backoff: u64,

    /// 静默模式: 禁止控制台输出 (与 --log-file 配合使用时日志仅写入文件)
    #[arg(short = 'q', long, default_value = "false", env = "QUIET")]
    pub quiet: bool,

    /// 日志文件路径: 将日志写入指定文件而非标准输出
    /// (可与 --quiet 配合实现纯文件日志)
    #[arg(long, env = "LOG_FILE")]
    pub log_file: Option<String>,
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
