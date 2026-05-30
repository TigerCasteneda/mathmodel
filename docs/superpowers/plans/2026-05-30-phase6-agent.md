# Phase 6: Local Agent + Claude Code Bridge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development

**Goal:** 本地 Agent 桥接 Claude Code CLI — WebSocket 透传终端 I/O、文件变更自动同步、Tabbit 数据接收

**Architecture:** 独立 Rust binary (`agent/`) 运行在队员电脑上，通过 WebSocket 连接平台服务器，管理 Claude Code 进程的 PTY，监听文件变更并推送到 CRDT sync 模块

---

## 非目标 (Phase 6a)

- 不实现 Tabbit 集成（留到 Phase 6b）
- 不实现 Agent 的 GUI，纯 CLI
- 不处理 Agent 离线重连时的 CRDT 冲突合并（用 yrs 默认行为）
- 不支持 Windows PTY（先只做 Unix PTY，Windows 用普通 stdin/stdout pipe）

---

## 架构概览

```
┌──────────────────────┐     WebSocket      ┌──────────────────────┐
│   Local Agent (Rust) │◄──────────────────►│   Modeler Server     │
│                      │                    │                      │
│  ┌────────────────┐  │  terminal_input    │  ┌────────────────┐  │
│  │ PTY Manager    │◄─┼────────────────────┼──│ agent_bridge/  │  │
│  │ (claude CLI)   │──┼────────────────────►│  │ handlers.rs    │  │
│  └────────────────┘  │  terminal_output   │  └───────┬────────┘  │
│                      │                    │          │           │
│  ┌────────────────┐  │  file_change       │          │ broadcast │
│  │ File Watcher   │──┼────────────────────►│  ┌───────▼────────┐  │
│  │ (notify crate) │  │                    │  │ sync::room     │  │
│  └────────────────┘  │                    │  │ (CRDT update)  │  │
│                      │                    │  └────────────────┘  │
└──────────────────────┘                    └──────────────────────┘
```

---

## 文件结构

```
agent/                          # 新的独立 Rust 项目
├── Cargo.toml
└── src/
    ├── main.rs                 # 入口、CLI 参数解析
    ├── ws_client.rs            # WebSocket 客户端 + 消息循环
    ├── pty_manager.rs          # PTY 进程管理 (claude CLI spawn/io)
    └── file_watcher.rs         # 文件变更监听 (notify crate)

server/src/agent_bridge/        # 平台端
├── mod.rs
├── handlers.rs                 # WS 端点、消息路由
```

---

## Task 1: Agent 项目创建 + 依赖

**Files:**
- Create: `agent/Cargo.toml`
- Create: `agent/src/main.rs`

- [ ] **Step 1: 创建 `agent/Cargo.toml`**

```toml
[package]
name = "modeler-agent"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
tokio-tungstenite = { version = "0.24", features = ["native-tls"] }
futures-util = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
notify = { version = "7", features = ["macos_kqueue"] }
clap = { version = "4", features = ["derive"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

- [ ] **Step 2: 创建 `agent/src/main.rs`** (CLI 入口)

```rust
mod ws_client;
mod pty_manager;
mod file_watcher;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "modeler-agent")]
struct Args {
    /// Modeler server WebSocket URL
    #[arg(long, default_value = "ws://localhost:3001/agent")]
    server: String,

    /// JWT token for authentication
    #[arg(long)]
    token: String,

    /// Project ID
    #[arg(long)]
    project_id: String,

    /// Local working directory to watch
    #[arg(long, default_value = ".")]
    work_dir: PathBuf,

    /// Path to claude CLI binary
    #[arg(long, default_value = "claude")]
    claude_path: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    tracing::info!("Starting Modeler Agent");
    tracing::info!("Server: {}", args.server);
    tracing::info!("Project: {}", args.project_id);
    tracing::info!("Work dir: {:?}", args.work_dir);

    ws_client::run(
        &args.server,
        &args.token,
        &args.project_id,
        &args.work_dir,
        &args.claude_path,
    ).await
}
```

---

## Task 2: Agent — WS Client + 消息循环

**Files:**
- Create: `agent/src/ws_client.rs`

WebSocket client that connects to the platform, sends/receives JSON messages, and dispatches to PTY/file watcher.

消息格式（JSON）：

```json
// Agent → Server
{ "type": "terminal_output", "data": "..." }
{ "type": "file_change", "path": "src/model.py", "content": "..." }
{ "type": "ready" }

// Server → Agent
{ "type": "terminal_input", "data": "pip install numpy\n" }
{ "type": "claude_command", "command": "write a SIR model" }
```

```rust
use crate::pty_manager::PtyManager;
use crate::file_watcher::FileWatcher;
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum AgentMessage {
    #[serde(rename = "terminal_input")]
    TerminalInput { data: String },
    #[serde(rename = "terminal_output")]
    TerminalOutput { data: String },
    #[serde(rename = "claude_command")]
    ClaudeCommand { command: String },
    #[serde(rename = "file_change")]
    FileChange { path: String, content: String },
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "error")]
    AgentError { message: String },
}

pub async fn run(
    server_url: &str,
    token: &str,
    project_id: &str,
    work_dir: &PathBuf,
    claude_path: &PathBuf,
) -> anyhow::Result<()> {
    let url = format!("{}?token={}&project_id={}", server_url, token, project_id);
    let (ws_stream, _) = connect_async(&url).await?;
    tracing::info!("Connected to server");

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Start PTY manager for Claude Code
    let mut pty = PtyManager::spawn(claude_path, work_dir)?;
    let pty = Arc::new(Mutex::new(pty));

    // Start file watcher
    let mut watcher = FileWatcher::new(work_dir)?;
    let watcher = Arc::new(Mutex::new(watcher));

    // Send ready
    let ready = AgentMessage::Ready;
    ws_tx.send(Message::Text(serde_json::to_string(&ready)?)).await?;

    // Main message loop
    loop {
        tokio::select! {
            // Incoming messages from server
            server_msg = ws_rx.next() => {
                match server_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<AgentMessage>(&text) {
                            match msg {
                                AgentMessage::TerminalInput { data } => {
                                    let mut p = pty.lock().await;
                                    p.write(&data)?;
                                }
                                AgentMessage::ClaudeCommand { command } => {
                                    let mut p = pty.lock().await;
                                    p.write(&format!("{}\n", command))?;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => continue,
                }
            }

            // PTY output from Claude Code
            pty_output = async {
                let mut p = pty.lock().await;
                p.read()
            } => {
                if let Some(data) = pty_output {
                    let msg = AgentMessage::TerminalOutput { data };
                    ws_tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
            }

            // File change events
            file_event = async {
                let mut w = watcher.lock().await;
                w.next_event().await
            } => {
                if let Some((path, content)) = file_event {
                    let msg = AgentMessage::FileChange { path, content };
                    ws_tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
            }
        }
    }

    tracing::info!("Agent disconnected");
    Ok(())
}
```

---

## Task 3: Agent — PTY Manager

**Files:**
- Create: `agent/src/pty_manager.rs`

在 Windows 上使用管道而非 PTY；Unix 上用 `portable-pty`。

```rust
use anyhow::Context;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct PtyManager {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::os::unix::io::RawFd, // Unix only for now
}

impl PtyManager {
    pub fn spawn(claude_path: &PathBuf, work_dir: &PathBuf) -> anyhow::Result<Self> {
        let mut child = Command::new(claude_path)
            .current_dir(work_dir)
            .env("CLAUDE_CODE_TERM", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn claude CLI")?;

        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        
        // For simplicity in Phase 6a, use stdio pipe (no real PTY).
        // Real PTY will be added in Phase 6b.

        Ok(PtyManager {
            child,
            stdin,
            stdout_fd: 0, // placeholder
        })
    }

    pub fn write(&mut self, data: &str) -> anyhow::Result<()> {
        self.stdin.write_all(data.as_bytes())?;
        self.stdin.flush()?;
        Ok(())
    }

    pub fn read(&mut self) -> Option<String> {
        use std::io::Read;
        let mut buf = [0u8; 4096];
        match self.child.stdout.as_mut()?.read(&mut buf) {
            Ok(0) => None,
            Ok(n) => Some(String::from_utf8_lossy(&buf[..n]).to_string()),
            Err(_) => None,
        }
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
```

---

## Task 4: Agent — File Watcher

**Files:**
- Create: `agent/src/file_watcher.rs`

```rust
use notify::{Event, RecursiveMode, Watcher, Config};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub struct FileWatcher {
    rx: mpsc::UnboundedReceiver<(String, String)>,
    _watcher: notify::RecommendedWatcher,
}

impl FileWatcher {
    pub fn new(work_dir: &PathBuf) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                for path in event.paths {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let rel = path.strip_prefix(work_dir)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();
                        let _ = tx.send((rel, content));
                    }
                }
            }
        }, Config::default())?;

        watcher.watch(work_dir, RecursiveMode::Recursive)?;

        Ok(FileWatcher { rx, _watcher: watcher })
    }

    pub async fn next_event(&mut self) -> Option<(String, String)> {
        self.rx.recv().await
    }
}
```

---

## Task 5: 平台端 — Agent Bridge WebSocket Handler

**Files:**
- Create: `server/src/agent_bridge/mod.rs`
- Create: `server/src/agent_bridge/handlers.rs`
- Update: `server/src/main.rs`

WebSocket 端点 `/agent` 接收 Agent 连接，转发消息。

```rust
// server/src/agent_bridge/handlers.rs

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, Query,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::{AppState, AppError};

#[derive(Debug, Deserialize)]
pub struct AgentQuery {
    pub token: String,
    pub project_id: String,
}

pub async fn agent_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<AgentQuery>,
) -> Result<impl IntoResponse, AppError> {
    // Verify JWT token
    use jsonwebtoken::{decode, DecodingKey, Validation};
    use crate::auth::model::Claims;

    let claims = decode::<Claims>(
        &query.token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid agent token".into()))?;

    let user_id = claims.claims.sub;
    let project_id = query.project_id;

    // Verify project membership
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?)"
    )
    .bind(&project_id).bind(&user_id)
    .fetch_one(&state.pool).await?;

    if exists == 0 {
        return Err(AppError::Forbidden("not a member".into()));
    }

    Ok(ws.on_upgrade(move |socket| handle_agent(socket, project_id, user_id, state.room_registry.clone(), state.pool.clone())))
}

async fn handle_agent(
    mut socket: WebSocket,
    project_id: String,
    user_id: String,
    registry: Arc<crate::sync::room::RoomRegistry>,
    pool: sqlx::SqlitePool,
) {
    tracing::info!("Agent connected: user={}, project={}", user_id, project_id);

    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    match value["type"].as_str() {
                        Some("terminal_output") => {
                            // Broadcast to frontend clients (via a separate channel — Phase 6b)
                            tracing::debug!("agent terminal: {}", value["data"]);
                        }
                        Some("file_change") => {
                            // Apply file change to CRDT sync room
                            let path = value["path"].as_str().unwrap_or("");
                            let content = value["content"].as_str().unwrap_or("");
                            tracing::info!("agent file change: {}", path);

                            // Find file_id from path, then push update
                            // For now, just log it
                        }
                        _ => {}
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    tracing::info!("Agent disconnected: user={}", user_id);
}
```

---

## Task 6: 集成 + 编译

- [ ] **Step 1: Update `server/src/main.rs`** 添加 agent_bridge 路由

```rust
mod agent_bridge;

// router:
.route("/agent", get(agent_bridge::handlers::agent_ws_handler))
```

- [ ] **Step 2: `cargo check`** — 验证平台端编译

- [ ] **Step 3: Agent 项目 `cargo check`**

```bash
cd agent && cargo check
```

---

## Task 7: E2E 测试

Agent 需要本地 Claude Code CLI 才能完整测试。基础验证：

```bash
# 启动服务器
cd server && cargo run

# 启动 agent (另一个终端)
cd agent && cargo run -- \
  --server ws://localhost:3001/agent \
  --token "$TOKEN" \
  --project_id "$PROJECT_ID" \
  --work_dir /tmp/modeler-test

# 验证：agent 连接成功、服务器日志出现 "Agent connected"
```
