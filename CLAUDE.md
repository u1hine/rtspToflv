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

_Project is in initial setup — no source modules exist yet._

The core pipeline is expected to involve:
- **RTSP client** — connecting to RTSP sources, handling RTP/RTCP transport
- **Demuxing** — parsing RTP payloads (likely H.264/H.265 video, possibly AAC audio)
- **FLV muxing** — remuxing elementary streams into FLV container format
- **HTTP server** — likely serving FLV over HTTP (HTTP-FLV) for web consumption

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
