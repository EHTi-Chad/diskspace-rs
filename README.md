# DiskSpace Analyzer

A fast, native disk space visualizer built with Rust and egui.

## Features

- Visual drive usage overview with color-coded usage bars
- Interactive donut pie chart with drill-down navigation
- File browser with breadcrumb navigation
- Drive details including SMART health data and I/O stats
- Auto-refresh every 30 seconds
- Cross-platform — builds on Windows, Linux, and macOS

## Building

Make sure you have Rust installed (https://rustup.rs), then:

```bash
cargo build --release
```

### Linux dependencies

```bash
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev pkg-config build-essential
```

## Running

```bash
cargo run --release
```

## License

EHTi Copyright 2026  
Built with help from Claude :)
