# rtspToflv

使用 Rust 语言将 RTSP 码流转封装为 HTTP-FLV 格式，供 Web 播放器 (微信小程序 live-player 等) 播放实时监控画面。

## 快速开始

```bash
# 编译
cargo build --release

# 运行 (替换为你的摄像头 RTSP 地址)
cargo run -- -u rtsp://admin:password@192.168.1.64:554/Streaming/Channels/101

# 或直接运行编译后的二进制
./target/release/rtsp-to-flv -u rtsp://admin:password@192.168.1.64:554/Streaming/Channels/101
```

播放地址: `http://127.0.0.1:8080/live/stream`

## 命令行参数

| 参数 | 简写 | 默认值 | 环境变量 | 说明 |
|------|------|--------|----------|------|
| `--rtsp-url` | `-u` | (必填) | `RTSP_URL` | RTSP 摄像头源地址 |
| `--port` | `-p` | `8080` | `HTTP_PORT` | HTTP 服务监听端口 |
| `--log-level` | `-l` | `info` | `LOG_LEVEL` | 日志等级 (trace/debug/info/warn/error) |
| `--reconnect-interval` | `-r` | `3` | `RECONNECT_INTERVAL` | RTSP 断连重试间隔 (秒) |
| `--stream-name` | `-n` | `stream` | `STREAM_NAME` | 流名称 (访问路径) |
| `--max-backoff` | — | `30` | `MAX_BACKOFF` | 最大重连退避时间 (秒) |

## 编译环境准备

### Windows

1. 安装 Rust: https://rustup.rs
2. 安装 Visual Studio Build Tools (C++ 生成工具)
3. `cargo build --release`

### Linux / macOS

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build --release
```

纯 Rust 实现，无额外系统依赖 (不需要安装 FFmpeg)。

## 微信小程序接入

```javascript
// 小程序 live-player 组件示例
<live-player
  src="http://192.168.1.100:8080/live/stream"
  mode="live"
  autoplay
  object-fit="fillCrop"
  style="width: 100vw; height: 100vh"
/>
```

注意:
- 小程序要求播放地址为 `http://` 非 `https://`（测试环境）
- 生产环境建议使用 nginx 反向代理添加 HTTPS
- 确保小程序服务器域名已配置

## 架构

```
IPC Camera (RTSP) → retina (RTP depacketize) → H.264 NALs / AAC frames
    → broadcast channel → FlvMuxer (FLV封装) → axum (HTTP-FLV) → 浏览器/小程序
```

- **RTSP 客户端**: retina (纯 Rust, 内置 H.264/AAC 解包)
- **FLV 封装**: oxideav-flv (纯 Rust)
- **HTTP 服务**: axum + tokio (异步, 高性能)
- **多客户端**: tokio::broadcast 多路复用，多个播放端共享一路 RTSP 连接

## 容错机制

- RTSP 断连自动重连，指数退避 (1s → 2s → 4s → ... → 30s)
- 成功重连后重置退避时间
- 无效 NAL 单元自动过滤
- 广播通道满时自动丢弃旧数据 (保护内存)

## 已知限制

- FLV 时间戳基于 RTP 时间戳转换 (90kHz → ms)，可能存在微秒级精度损失
- 不支持 H.265/HEVC 编码的摄像头 (retina 支持但 FLV 封装需扩展)
- 当前仅支持第一个视频流和第一个音频流
- 未实现 seeking / 回放功能 (直播流)
