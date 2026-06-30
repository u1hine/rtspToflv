# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RTSP to FLV stream converter written in Rust. Converts RTSP (Real Time Streaming Protocol) video streams to FLV (Flash Video) format.

## Build System

This is a Rust/Cargo project.

```bash
cargo build          # Debug build
cargo build --release # Release build
cargo run            # Run the binary
cargo test           # Run all tests
cargo test <name>    # Run a single test by name
cargo clippy         # Lint
cargo fmt            # Format code
```

## Architecture

```
IPC Camera (RTSP) → retina (RTP depacketize) → H.264 NALs / AAC frames
    → broadcast channel → FlvMuxer (FLV封装) → axum (HTTP-FLV) → 浏览器/小程序
```

### Module Structure

| Module | File | Purpose |
|--------|------|---------|
| Entry point | `src/main.rs` | CLI args, logging init, startup orchestration |
| Config | `src/config.rs` | `Config` struct with clap derive — RTSP URL, port, log level, reconnect params |
| Error | `src/error.rs` | `AppError` enum (Rtsp/Flv/Server/BroadcastClosed) via thiserror |
| RTSP client | `src/rtsp/client.rs` | `RtspClient` — retina-based RTSP pull, auto-reconnect with exponential backoff |
| FLV muxer | `src/flv/muxer.rs` | `FlvMuxer` — wraps oxideav-flv for header, metadata, video/audio tags |
| HTTP server | `src/server/handler.rs` | `GET /live/:stream_name` — chunked HTTP-FLV streaming endpoint |
| Shared state | `src/server/state.rs` | `AppState` — holds `tokio::sync::broadcast::Sender<StreamMessage>` |

### Key Dependencies

- **retina** 0.4 — pure-Rust RTSP client with built-in H.264/AAC depacketization
- **oxideav-flv** 0.0.5 — pure-Rust FLV muxer (header, script, video/audio tags)
- **axum** 0.8 — HTTP framework for HTTP-FLV streaming endpoint
- **tokio** 1 — async runtime, broadcast channel for multi-client

### Data Flow

1. `RtspClient` connects via `Session::describe()` → `setup()` → `play()` → `demuxed()`
2. `Demuxed` (implements `futures::Stream`) yields `CodecItem::VideoFrame` / `AudioFrame`
3. Raw frame data + timestamps wrapped in `StreamMessage` enum and sent via `broadcast::Sender`
4. Each HTTP client subscribes via `broadcast::Receiver`, independently runs `FlvMuxer` to produce FLV tags
5. FLV tags written directly to HTTP response body as chunked `video/x-flv`

### Usage

```bash
cargo run -- -u rtsp://admin:password@192.168.1.64:554/Streaming/Channels/101
# Playback: http://127.0.0.1:8080/live/stream
```

## Skill routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill
tool as your FIRST action. Do NOT answer directly, do NOT use other tools first.
The skill has specialized workflows that produce better results than ad-hoc answers.

Key routing rules:
- Product ideas, "is this worth building", brainstorming → invoke office-hours
- Bugs, errors, "why is this broken", 500 errors → invoke investigate
- Ship, deploy, push, create PR → invoke ship
- QA, test the site, find bugs → invoke qa
- Code review, check my diff → invoke review
- Update docs after shipping → invoke document-release
- Weekly retro → invoke retro
- Design system, brand → invoke design-consultation
- Visual audit, design polish → invoke design-review
- Architecture review → invoke plan-eng-review
- Save progress, checkpoint, resume → invoke checkpoint
- Code quality, health check → invoke health
