# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 语言

使用中文回复，代码注释使用英文。

## 项目概要

OpenConnect VPN 的跨平台桌面客户端，Rust + Tauri + React/NextUI 构建。支持密码和 OIDC 两种认证方式，提供 GUI 和 CLI 两套界面。

## 构建命令

```bash
# 构建整个 workspace
cargo build

# 构建特定 crate
cargo build -p openconnect-core

# 运行所有测试
cargo test

# 运行特定 crate 测试
cargo test -p openconnect-core

# 前端开发（先在 crates/openconnect-gui 下安装 pnpm 依赖）
cd crates/openconnect-gui && pnpm install && pnpm dev

# Tauri 开发模式（前端 dev server + Tauri 窗口）
cd crates/openconnect-gui && pnpm tauri dev
```

**环境要求**: 依赖 OpenConnect C 库。默认使用预编译的静态库（从 SourceForge 下载），通过 `.cargo/config.toml` 中 `OPENCONNECT_USE_PREBUILT=true` 控制。如需从源码编译，设为 `false` 并参考 `crates/openconnect-sys/README.md` 安装依赖。

macOS 上还需安装 `openssl`、`libxml2`、`zlib`、`pkg-config`。Windows 需要 MSYS2 MINGW64 + `x86_64-pc-windows-gnu` toolchain。

## 架构

### Workspace 结构（5 个 crate，按依赖层级排列）

| Crate | 定位 |
|---|---|
| `openconnect-sys` | FFI 层，通过 bindgen 绑定 OpenConnect C 库。含平台特定预生成 bindings（`src/bindings_*.rs`），build.rs 负责下载预编译库或从源码编译，同时编译 `c-src/helper.c` 辅助函数 |
| `openconnect-core` | 安全 Rust 封装。核心是 `VpnClient` 结构体（包装 `*mut openconnect_info`），实现 `Connectable` trait 管理完整生命周期：`new` → `init_connection` → `run_loop` → `disconnect` |
| `openconnect-oidc` | OIDC 认证（MIT 协议），处理 token 换取 VPN cookie |
| `openconnect-cli` | CLI 客户端。守护进程架构：`start` 命令 fork 到后台后通过 Unix domain socket（JSON 帧协议）与前台命令通信。子命令包括 `add/delete/list/start/stop/status/logs/import/export` |
| `openconnect-gui` | Tauri 桌面应用。前端 React 18 + NextUI + Jotai 状态管理 + Tailwind。`src-tauri/src/` 中 Rust 端通过 Tauri command 调用 openconnect-core |

### 认证流程

- **密码认证**: OpenConnect 的 HTML form 流程 — 先提交 username，再提交 password，获取 cookie 后建立 CSTP 连接
- **OIDC 认证**: 通过系统浏览器完成外部 OIDC 流程（`oidcvpn` 自定义 URI scheme 回调），获得 token 后调用 `openconnect-oidc` 换取 VPN cookie

### 配置存储

JSON 文件 `~/.oidcvpn/config.json`，密码使用 XChaCha20Poly1305 加密（密钥由 machine-uid 派生），存储结构见 `openconnect-core/src/storage.rs` 的 `StoredConfigs`。

### 关键模块（openconnect-core）

| 文件 | 职责 |
|---|---|
| `lib.rs` | `VpnClient` 定义 + `Connectable` trait 实现 |
| `config.rs` | `Config`/`Entrypoint` Builder 模式 |
| `events.rs` | 事件回调：连接状态变更、证书验证 |
| `form.rs` | 处理 OpenConnect auth form XML 解析与提交 |
| `command.rs` | cmd_pipe 通信（向 C 层主循环发送 cancel 等命令） |
| `storage.rs` | 服务器配置 JSON 持久化 + 密码加解密 |
| `protocols.rs` | VPN 协议类型定义（AnyConnect 等） |
| `ip_info.rs` | 连接后的 IP 地址信息 |
| `log.rs` | tracing 日志初始化，文件滚动 |

### 提权机制

CLI 在 macOS/Linux 上使用 `sudo` crate 提权。GUI 在 macOS 上通过 `openconnect-core::elevator::macos` 提权（使用 Security framework 创建特权 helper），Windows 上通过 ShellExecute runas。

### 测试

- Rust 单元测试分散在各 crate 的 `src/` 中（`#[test]` 和 `#[tokio::test]`）
- `test/` 目录包含一个 Express mock VPN 服务器，用于手动测试认证流程
