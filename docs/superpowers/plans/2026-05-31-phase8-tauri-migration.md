# Phase 8 Tauri Migration — Agent-First Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate Modeler AI to a fully self-contained Tauri desktop app where all frontend-backend communication uses Tauri invoke/event (replacing fetch/WebSocket).

**Architecture:** Phase 8b ports the standalone Agent binary (PTY, file watching, file tree) into the Tauri Rust backend as Tauri commands + events. Phase 8c replaces the frontend's agent WebSocket code with `@tauri-apps/api` invoke/listen. Phase 8d wraps remaining server HTTP calls with Tauri invoke. Phase 8e embeds the Axum server in-process so the app runs fully self-contained.

**Tech Stack:** Tauri 2.x, Rust (tokio, portable-pty, notify, serde), Next.js 16 static export, TypeScript, @tauri-apps/api

---

## File Structure

```
src-tauri/
  src/
    lib.rs                          # MODIFIED — register commands, manage AgentState
    main.rs                         # UNCHANGED
    agent/
      mod.rs                        # NEW — module declarations
      pty.rs                        # NEW — PTY manager (ported from agent/src/pty_manager.rs)
      file_watcher.rs               # NEW — file watcher + tree scanner (ported from agent/src/file_watcher.rs)
      commands.rs                   # NEW — Tauri #[command] functions
      state.rs                      # NEW — AgentState managed by Tauri
      events.rs                     # NEW — event payload structs
  Cargo.toml                        # MODIFIED — add agent deps (notify, portable-pty, etc.)

hooks/
  use-tauri-agent.ts                # NEW — agent hook using Tauri invoke/listen

components/dashboard/
  code-canvas.tsx                   # MODIFIED — replace agent WebSocket with Tauri API

lib/
  tauri-api.ts                      # NEW — typed Tauri invoke wrappers
```

---

### Task 1: Add agent dependencies to src-tauri/Cargo.toml

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Uncomment and add agent dependencies**

Replace the commented Agent deps block in `src-tauri/Cargo.toml` with active dependencies:

```toml
# Agent deps (Phase 8b)
notify = "7"
portable-pty = "0.9"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

Full `[dependencies]` section:

```toml
[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }

# Agent deps (Phase 8b)
notify = "7"
portable-pty = "0.9"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"

# Server deps for Host Mode (Phase 8e — commented)
# modeler-server = { path = "../server" }
```

- [ ] **Step 2: Verify Cargo.toml parses**

Run: `cd src-tauri && cargo check 2>&1 | head -20`
Expected: Dependencies resolve (may warn about unused deps — that's fine for now).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "feat: add agent dependencies to Tauri (Phase 8b)"
```

---

### Task 2: Create agent state module

**Files:**
- Create: `src-tauri/src/agent/state.rs`
- Create: `src-tauri/src/agent/mod.rs`

- [ ] **Step 1: Create src-tauri/src/agent/state.rs**

This holds the PTY sender and current working directory, managed by Tauri's state system.

```rust
use portable_pty::PtySize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tokio::sync::mpsc;

/// Commands sent to the PTY manager task.
pub enum PtyCommand {
    Input(String),
    Resize { cols: u16, rows: u16 },
    Kill,
}

/// Managed Tauri state for the local agent.
pub struct AgentState {
    /// Sender to the PTY manager task. None if no PTY is running.
    pub pty_tx: Mutex<Option<mpsc::UnboundedSender<PtyCommand>>>,
    /// Current working directory for file operations.
    pub work_dir: Mutex<PathBuf>,
    /// AppHandle for emitting events from background tasks.
    pub app_handle: AppHandle,
}
```

- [ ] **Step 2: Create src-tauri/src/agent/mod.rs**

```rust
pub mod commands;
pub mod events;
pub mod file_watcher;
pub mod pty;
pub mod state;
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/agent/mod.rs src-tauri/src/agent/state.rs
git commit -m "feat: add AgentState and agent module skeleton (Phase 8b)"
```

---

### Task 3: Port PTY manager to Tauri

**Files:**
- Create: `src-tauri/src/agent/pty.rs`
- Modify: `src-tauri/src/agent/mod.rs` (already done in Task 2)

- [ ] **Step 1: Create src-tauri/src/agent/pty.rs**

Ported from `agent/src/pty_manager.rs` — same logic, adapted to use AgentState's channel types and emit events via AppHandle.

```rust
use crate::agent::events::AgentEvent;
use crate::agent::state::{AgentState, PtyCommand};
use anyhow::Context;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Spawn a PTY shell, forwarding output to the frontend via Tauri events.
/// Runs the blocking PTY I/O on a dedicated OS thread.
pub async fn spawn_pty(
    work_dir: PathBuf,
    mut command_rx: mpsc::UnboundedReceiver<PtyCommand>,
    state: Arc<AgentState>,
) -> anyhow::Result<()> {
    let app_handle = state.app_handle.clone();

    tokio::task::spawn_blocking(move || {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open PTY")?;

        let mut command = shell_command();
        command.cwd(&work_dir);
        let mut child = pair
            .slave
            .spawn_command(command)
            .context("Failed to spawn shell in PTY")?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().context("no PTY reader")?;
        let mut writer = pair.master.take_writer().context("no PTY writer")?;

        // Reader thread
        let read_app = app_handle.clone();
        let read_thread = std::thread::spawn(move || {
            let mut buf = vec![0; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = read_app.emit("pty-output", AgentEvent::PtyOutput { data });
                    }
                    Err(err) => {
                        let _ = read_app.emit(
                            "agent-error",
                            AgentEvent::AgentError {
                                message: format!("PTY read error: {err}"),
                            },
                        );
                        break;
                    }
                }
            }
        });

        // Command loop
        while let Some(cmd) = command_rx.blocking_recv() {
            match cmd {
                PtyCommand::Input(data) => {
                    if let Err(err) = writer.write_all(data.as_bytes()) {
                        let _ = app_handle.emit(
                            "agent-error",
                            AgentEvent::AgentError {
                                message: format!("PTY write error: {err}"),
                            },
                        );
                        break;
                    }
                    let _ = writer.flush();
                }
                PtyCommand::Resize { cols, rows } => {
                    let _ = pair.master.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
                PtyCommand::Kill => break,
            }
        }

        let _ = child.kill();
        let _ = child.wait();
        let _ = read_thread.join();
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("PTY task join failed")??;

    Ok(())
}

fn shell_command() -> CommandBuilder {
    #[cfg(windows)]
    {
        for shell in ["pwsh.exe", "powershell.exe", "cmd.exe"] {
            if command_exists(shell) {
                let mut cmd = CommandBuilder::new(shell);
                if shell == "cmd.exe" {
                    cmd.arg("/Q");
                }
                return cmd;
            }
        }
        CommandBuilder::new("cmd.exe")
    }

    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-i");
        cmd
    }
}

#[cfg(windows)]
fn command_exists(command: &str) -> bool {
    std::process::Command::new("where")
        .arg(command)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/agent/pty.rs
git commit -m "feat: port PTY manager to Tauri (Phase 8b)"
```

---

### Task 4: Port file watcher to Tauri

**Files:**
- Create: `src-tauri/src/agent/file_watcher.rs`

- [ ] **Step 1: Create src-tauri/src/agent/file_watcher.rs**

Ported from `agent/src/file_watcher.rs` — same logic unchanged. These are pure functions with no WebSocket dependency.

```rust
use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub struct FileWatcher {
    rx: mpsc::UnboundedReceiver<(String, String)>,
    _watcher: notify::RecommendedWatcher,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTreeItem {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileTreeItem>>,
}

impl FileWatcher {
    pub fn new(work_dir: &PathBuf) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let wd = work_dir.clone();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if !matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) {
                    return;
                }
                for path in event.paths {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let rel = path
                            .strip_prefix(&wd)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .replace('\\', "/");
                        let _ = tx.send((rel, content));
                    }
                }
            }
        })?;

        watcher.watch(work_dir, RecursiveMode::Recursive)?;
        Ok(FileWatcher {
            rx,
            _watcher: watcher,
        })
    }

    pub async fn next_event(&mut self) -> Option<(String, String)> {
        self.rx.recv().await
    }
}

pub fn scan_tree(work_dir: &PathBuf) -> anyhow::Result<FileTreeItem> {
    let root_name = work_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    scan_path(work_dir, work_dir, root_name)
}

pub fn read_workspace_file(work_dir: &PathBuf, path: &str) -> anyhow::Result<String> {
    let full_path = work_dir.join(path);
    let full_path = full_path.canonicalize()?;
    let root = work_dir.canonicalize()?;
    if !full_path.starts_with(root) || !full_path.is_file() {
        anyhow::bail!("file is outside workspace or not a file");
    }
    Ok(std::fs::read_to_string(full_path)?)
}

fn scan_path(root: &PathBuf, path: &PathBuf, name: String) -> anyhow::Result<FileTreeItem> {
    let rel_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    if path.is_dir() {
        let mut children = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let child_path = entry.path();
            let child_name = entry.file_name().to_string_lossy().to_string();
            if should_skip(&child_name) {
                continue;
            }
            if let Ok(child) = scan_path(root, &child_path, child_name) {
                children.push(child);
            }
        }
        children.sort_by(|a, b| {
            b.node_type
                .cmp(&a.node_type)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        return Ok(FileTreeItem {
            name,
            path: rel_path,
            node_type: "folder".into(),
            language: None,
            children: Some(children),
        });
    }

    Ok(FileTreeItem {
        name: name.clone(),
        path: rel_path,
        node_type: "file".into(),
        language: language_for_name(&name),
        children: None,
    })
}

fn should_skip(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".next" | "node_modules" | "target" | ".DS_Store"
    )
}

fn language_for_name(name: &str) -> Option<String> {
    let ext = name.rsplit('.').next().unwrap_or("");
    let lang = match ext {
        "py" => "python",
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "json" => "json",
        "md" => "markdown",
        "tex" => "latex",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "txt" => "plaintext",
        _ => "plaintext",
    };
    Some(lang.into())
}
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/agent/file_watcher.rs
git commit -m "feat: port file watcher and tree scanner to Tauri (Phase 8b)"
```

---

### Task 5: Create event payload types

**Files:**
- Create: `src-tauri/src/agent/events.rs`

- [ ] **Step 1: Create src-tauri/src/agent/events.rs**

```rust
use crate::agent::file_watcher::FileTreeItem;
use serde::{Deserialize, Serialize};

/// Events emitted from the Tauri backend to the frontend.
/// Each variant is emitted as a separate Tauri event name for type-safe frontend listening.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "pty_output")]
    PtyOutput { data: String },
    #[serde(rename = "agent_error")]
    AgentError { message: String },
    #[serde(rename = "file_change")]
    FileChange { path: String, content: String },
    #[serde(rename = "file_tree")]
    FileTree { tree: FileTreeItem },
    #[serde(rename = "file_content")]
    FileContent { path: String, content: String },
    #[serde(rename = "work_dir")]
    WorkDir { path: String },
}
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/agent/events.rs
git commit -m "feat: add Tauri event payload types for agent (Phase 8b)"
```

---

### Task 6: Create Tauri commands

**Files:**
- Create: `src-tauri/src/agent/commands.rs`

- [ ] **Step 1: Create src-tauri/src/agent/commands.rs**

These are the `#[tauri::command]` functions the frontend will invoke. Each corresponds to a message type that was previously sent over the agent WebSocket.

```rust
use crate::agent::events::AgentEvent;
use crate::agent::file_watcher::{self, FileTreeItem};
use crate::agent::pty;
use crate::agent::state::{AgentState, PtyCommand};
use std::path::{Component, PathBuf};
use tauri::State;
use tokio::sync::mpsc;

fn validate_create_path(work_dir: &std::path::Path, relative_path: &str) -> Result<PathBuf, String> {
    let rel = std::path::Path::new(relative_path);
    if rel.is_absolute() {
        return Err("absolute path rejected".into());
    }
    for component in rel.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("path traversal rejected".into());
            }
            _ => {}
        }
    }
    let resolved = work_dir.join(rel);
    let canon_work = work_dir.canonicalize().unwrap_or_else(|_| work_dir.to_path_buf());
    if let Ok(canon) = resolved.canonicalize() {
        if !canon.starts_with(&canon_work) {
            return Err("path escapes workspace".into());
        }
    } else if !resolved.starts_with(&canon_work) {
        return Err("path escapes workspace".into());
    }
    Ok(resolved)
}

#[tauri::command]
pub async fn pty_spawn(
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let (tx, rx) = mpsc::unbounded_channel();
    {
        let mut pty_tx = state.pty_tx.lock().map_err(|e| e.to_string())?;
        // Kill existing PTY if any
        if let Some(old_tx) = pty_tx.take() {
            let _ = old_tx.send(PtyCommand::Kill);
        }
        *pty_tx = Some(tx);
    }

    // We need Arc<AgentState> for the background task, but State<'_, AgentState>
    // is already a reference to Tauri-managed state. We pass the AppHandle instead.
    let app_handle = state.app_handle.clone();
    tokio::spawn(async move {
        // Create a lightweight state wrapper for the spawned task
        let task_state = std::sync::Arc::new(AgentState {
            pty_tx: std::sync::Mutex::new(None),
            work_dir: std::sync::Mutex::new(work_dir.clone()),
            app_handle: app_handle.clone(),
        });
        if let Err(err) = pty::spawn_pty(work_dir, rx, task_state).await {
            let _ = app_handle.emit(
                "agent-error",
                AgentEvent::AgentError {
                    message: format!("PTY spawn failed: {err:#}"),
                },
            );
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn pty_write(
    data: String,
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let pty_tx = state.pty_tx.lock().map_err(|e| e.to_string())?;
    match pty_tx.as_ref() {
        Some(tx) => {
            tx.send(PtyCommand::Input(data))
                .map_err(|_| "PTY has closed".to_string())
        }
        None => Err("No PTY session. Call pty_spawn first.".to_string()),
    }
}

#[tauri::command]
pub async fn pty_resize(
    cols: u16,
    rows: u16,
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let pty_tx = state.pty_tx.lock().map_err(|e| e.to_string())?;
    match pty_tx.as_ref() {
        Some(tx) => {
            tx.send(PtyCommand::Resize { cols, rows })
                .map_err(|_| "PTY has closed".to_string())
        }
        None => Err("No PTY session. Call pty_spawn first.".to_string()),
    }
}

#[tauri::command]
pub async fn pty_kill(
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let mut pty_tx = state.pty_tx.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = pty_tx.take() {
        let _ = tx.send(PtyCommand::Kill);
    }
    Ok(())
}

#[tauri::command]
pub async fn list_files(
    state: State<'_, AgentState>,
) -> Result<FileTreeItem, String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    file_watcher::scan_tree(&work_dir).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn read_file(
    path: String,
    state: State<'_, AgentState>,
) -> Result<String, String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    file_watcher::read_workspace_file(&work_dir, &path).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn create_file(
    path: String,
    content: String,
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let resolved = validate_create_path(&work_dir, &path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{e:#}"))?;
    }
    if resolved.exists() {
        return Err("file already exists".into());
    }
    std::fs::write(&resolved, &content).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn change_work_dir(
    path: String,
    state: State<'_, AgentState>,
) -> Result<FileTreeItem, String> {
    let new_dir = PathBuf::from(&path);
    if !new_dir.is_dir() {
        return Err("path is not a directory".into());
    }
    {
        let mut work_dir = state.work_dir.lock().map_err(|e| e.to_string())?;
        *work_dir = new_dir.clone();
    }
    // Emit work_dir changed event
    let _ = state.app_handle.emit(
        "work-dir",
        AgentEvent::WorkDir {
            path: path.clone(),
        },
    );
    // Return the new file tree
    let tree = file_watcher::scan_tree(&new_dir).map_err(|e| format!("{e:#}"))?;
    let _ = state.app_handle.emit(
        "file-tree",
        AgentEvent::FileTree {
            tree: tree.clone(),
        },
    );
    Ok(tree)
}
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/agent/commands.rs
git commit -m "feat: add Tauri commands for agent (pty, files, work_dir) (Phase 8b)"
```

---

### Task 7: Register commands and state in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Rewrite src-tauri/src/lib.rs**

```rust
mod agent;

use agent::state::AgentState;
use std::path::PathBuf;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            app.manage(AgentState {
                pty_tx: std::sync::Mutex::new(None),
                work_dir: std::sync::Mutex::new(work_dir),
                app_handle: app.handle().clone(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent::commands::pty_spawn,
            agent::commands::pty_write,
            agent::commands::pty_resize,
            agent::commands::pty_kill,
            agent::commands::list_files,
            agent::commands::read_file,
            agent::commands::create_file,
            agent::commands::change_work_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 2: Build check**

Run: `cd src-tauri && cargo check 2>&1`
Expected: Compilation succeeds with no errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: register agent commands and state in Tauri builder (Phase 8b)"
```

---

### Task 8: Create typed Tauri API wrappers for frontend

**Files:**
- Create: `lib/tauri-api.ts`

- [ ] **Step 1: Create lib/tauri-api.ts**

Type-safe wrappers around `@tauri-apps/api` invoke and listen. Provides a clean API for React components and handles the fallback case when not running in Tauri.

```typescript
"use client"

import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface FileTreeItem {
  name: string
  path: string
  type: "file" | "folder"
  language?: string
  children?: FileTreeItem[]
}

export interface PtyOutputEvent {
  type: "pty_output"
  data: string
}

export interface AgentErrorEvent {
  type: "agent_error"
  message: string
}

export interface FileChangeEvent {
  type: "file_change"
  path: string
  content: string
}

export interface FileTreeEvent {
  type: "file_tree"
  tree: FileTreeItem
}

export interface FileContentEvent {
  type: "file_content"
  path: string
  content: string
}

export interface WorkDirEvent {
  type: "work_dir"
  path: string
}

export type AgentEvent =
  | PtyOutputEvent
  | AgentErrorEvent
  | FileChangeEvent
  | FileTreeEvent
  | FileContentEvent
  | WorkDirEvent

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
}

// ─── Commands ───────────────────────────────────────

export async function ptySpawn(): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_spawn")
}

export async function ptyWrite(data: string): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_write", { data })
}

export async function ptyResize(cols: number, rows: number): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_resize", { cols, rows })
}

export async function ptyKill(): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_kill")
}

export async function listFiles(): Promise<FileTreeItem> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<FileTreeItem>("list_files")
}

export async function readFile(path: string): Promise<string> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<string>("read_file", { path })
}

export async function createFile(path: string, content: string): Promise<void> {
  if (!isTauri()) return
  return invoke("create_file", { path, content })
}

export async function changeWorkDir(path: string): Promise<FileTreeItem> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<FileTreeItem>("change_work_dir", { path })
}

// ─── Events ─────────────────────────────────────────

export function onPtyOutput(callback: (data: string) => void): UnlistenFn {
  const unlisten = listen<PtyOutputEvent>("pty-output", (event) => {
    callback(event.payload.data)
  })
  // Return a function that calls the internal unlisten
  // listen() returns a Promise<UnlistenFn>, so we handle it async
  let cancelled = false
  unlisten.then((fn) => {
    if (cancelled) fn()
  })
  return () => {
    cancelled = true
    unlisten.then((fn) => fn())
  }
}

export function onAgentError(callback: (message: string) => void): UnlistenFn {
  const unlisten = listen<AgentErrorEvent>("agent-error", (event) => {
    callback(event.payload.message)
  })
  let cancelled = false
  unlisten.then((fn) => {
    if (cancelled) fn()
  })
  return () => {
    cancelled = true
    unlisten.then((fn) => fn())
  }
}

export function onFileChange(callback: (path: string, content: string) => void): UnlistenFn {
  const unlisten = listen<FileChangeEvent>("file-change", (event) => {
    callback(event.payload.path, event.payload.content)
  })
  let cancelled = false
  unlisten.then((fn) => {
    if (cancelled) fn()
  })
  return () => {
    cancelled = true
    unlisten.then((fn) => fn())
  }
}

export function onFileTree(callback: (tree: FileTreeItem) => void): UnlistenFn {
  const unlisten = listen<FileTreeEvent>("file-tree", (event) => {
    callback(event.payload.tree)
  })
  let cancelled = false
  unlisten.then((fn) => {
    if (cancelled) fn()
  })
  return () => {
    cancelled = true
    unlisten.then((fn) => fn())
  }
}

export function onFileContent(callback: (path: string, content: string) => void): UnlistenFn {
  const unlisten = listen<FileContentEvent>("file-content", (event) => {
    callback(event.payload.path, event.payload.content)
  })
  let cancelled = false
  unlisten.then((fn) => {
    if (cancelled) fn()
  })
  return () => {
    cancelled = true
    unlisten.then((fn) => fn())
  }
}

export function onWorkDirChanged(callback: (path: string) => void): UnlistenFn {
  const unlisten = listen<WorkDirEvent>("work-dir", (event) => {
    callback(event.payload.path)
  })
  let cancelled = false
  unlisten.then((fn) => {
    if (cancelled) fn()
  })
  return () => {
    cancelled = true
    unlisten.then((fn) => fn())
  }
}

// ─── Combined hook helper ───────────────────────────

export function subscribeAgent(options: {
  onPtyOutput?: (data: string) => void
  onError?: (message: string) => void
  onFileChange?: (path: string, content: string) => void
  onFileTree?: (tree: FileTreeItem) => void
  onFileContent?: (path: string, content: string) => void
  onWorkDir?: (path: string) => void
}): () => void {
  if (!isTauri()) return () => {}

  const unlisteners: UnlistenFn[] = []

  if (options.onPtyOutput) unlisteners.push(onPtyOutput(options.onPtyOutput))
  if (options.onError) unlisteners.push(onAgentError(options.onError))
  if (options.onFileChange) unlisteners.push(onFileChange(options.onFileChange))
  if (options.onFileTree) unlisteners.push(onFileTree(options.onFileTree))
  if (options.onFileContent) unlisteners.push(onFileContent(options.onFileContent))
  if (options.onWorkDir) unlisteners.push(onWorkDirChanged(options.onWorkDir))

  return () => {
    unlisteners.forEach((fn) => fn())
  }
}
```

Wait, the `listen()` return type in `@tauri-apps/api` v2 is `Promise<UnlistenFn>`. Let me fix the event functions to handle this properly:

```typescript
"use client"

import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface FileTreeItem {
  name: string
  path: string
  type: "file" | "folder"
  language?: string
  children?: FileTreeItem[]
}

export interface PtyOutputEvent {
  type: "pty_output"
  data: string
}

export interface AgentErrorEvent {
  type: "agent_error"
  message: string
}

export interface FileChangeEvent {
  type: "file_change"
  path: string
  content: string
}

export interface FileTreeEvent {
  type: "file_tree"
  tree: FileTreeItem
}

export interface FileContentEvent {
  type: "file_content"
  path: string
  content: string
}

export interface WorkDirEvent {
  type: "work_dir"
  path: string
}

export type AgentEvent =
  | PtyOutputEvent
  | AgentErrorEvent
  | FileChangeEvent
  | FileTreeEvent
  | FileContentEvent
  | WorkDirEvent

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
}

// ─── Commands ───────────────────────────────────────

export async function ptySpawn(): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_spawn")
}

export async function ptyWrite(data: string): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_write", { data })
}

export async function ptyResize(cols: number, rows: number): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_resize", { cols, rows })
}

export async function ptyKill(): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_kill")
}

export async function listFiles(): Promise<FileTreeItem> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<FileTreeItem>("list_files")
}

export async function readFile(path: string): Promise<string> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<string>("read_file", { path })
}

export async function createFile(path: string, content: string): Promise<void> {
  if (!isTauri()) return
  return invoke("create_file", { path, content })
}

export async function changeWorkDir(path: string): Promise<FileTreeItem> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<FileTreeItem>("change_work_dir", { path })
}

// ─── Events ─────────────────────────────────────────

function listenEvent<T>(event: string, callback: (payload: T) => void): () => void {
  let cancelled = false
  let unlisten: UnlistenFn | null = null

  listen<T>(event, (e) => {
    if (!cancelled) callback(e.payload)
  }).then((fn) => {
    if (cancelled) fn()
    else unlisten = fn
  })

  return () => {
    cancelled = true
    unlisten?.()
  }
}

export function onPtyOutput(callback: (data: string) => void): () => void {
  return listenEvent<PtyOutputEvent>("pty-output", (e) => callback(e.data))
}

export function onAgentError(callback: (message: string) => void): () => void {
  return listenEvent<AgentErrorEvent>("agent-error", (e) => callback(e.message))
}

export function onFileChange(callback: (path: string, content: string) => void): () => void {
  return listenEvent<FileChangeEvent>("file-change", (e) => callback(e.path, e.content))
}

export function onFileTree(callback: (tree: FileTreeItem) => void): () => void {
  return listenEvent<FileTreeEvent>("file-tree", (e) => callback(e.tree))
}

export function onFileContent(callback: (path: string, content: string) => void): () => void {
  return listenEvent<FileContentEvent>("file-content", (e) => callback(e.path, e.content))
}

export function onWorkDirChanged(callback: (path: string) => void): () => void {
  return listenEvent<WorkDirEvent>("work-dir", (e) => callback(e.path))
}
```

- [ ] **Step 2: Commit**

```bash
git add lib/tauri-api.ts
git commit -m "feat: add typed Tauri API wrappers for frontend (Phase 8c)"
```

---

### Task 9: Create useTauriAgent hook

**Files:**
- Create: `hooks/use-tauri-agent.ts`

- [ ] **Step 1: Create hooks/use-tauri-agent.ts**

A React hook that encapsulates all Tauri agent communication, replacing the WebSocket-based agent code in code-canvas.tsx.

```typescript
"use client"

import { useState, useEffect, useRef, useCallback } from "react"
import type { FileTreeItem } from "@/lib/tauri-api"
import * as tauriApi from "@/lib/tauri-api"

export type AgentStatus = "connecting" | "connected" | "ready" | "disconnected"

export function useTauriAgent(initialWorkDir?: string) {
  const [status, setStatus] = useState<AgentStatus>("disconnected")
  const [fileTree, setFileTree] = useState<FileTreeItem | null>(null)
  const [fileContents, setFileContents] = useState<Record<string, string>>({})
  const [workDir, setWorkDir] = useState<string | null>(initialWorkDir || null)
  const [terminalOutput, setTerminalOutput] = useState("")
  const terminalCallbackRef = useRef<((data: string) => void) | null>(null)
  const cleanupRef = useRef<(() => void) | null>(null)

  const connect = useCallback(async () => {
    setStatus("connecting")

    try {
      await tauriApi.ptySpawn()
      setStatus("ready")

      // Subscribe to all agent events
      const unlisteners: (() => void)[] = []

      unlisteners.push(
        tauriApi.onPtyOutput((data) => {
          setTerminalOutput((prev) => prev + data)
          terminalCallbackRef.current?.(data)
        })
      )

      unlisteners.push(
        tauriApi.onAgentError((message) => {
          setTerminalOutput((prev) => prev + `\r\n[agent] ${message}\r\n`)
        })
      )

      unlisteners.push(
        tauriApi.onFileChange((path, content) => {
          setFileContents((prev) => ({ ...prev, [path]: content }))
        })
      )

      unlisteners.push(
        tauriApi.onFileTree((tree) => {
          setFileTree(tree)
        })
      )

      unlisteners.push(
        tauriApi.onFileContent((path, content) => {
          setFileContents((prev) => ({ ...prev, [path]: content }))
        })
      )

      unlisteners.push(
        tauriApi.onWorkDirChanged((path) => {
          setWorkDir(path)
        })
      )

      cleanupRef.current = () => {
        unlisteners.forEach((fn) => fn())
      }

      // Load initial file tree
      const tree = await tauriApi.listFiles()
      setFileTree(tree)
    } catch (err) {
      setStatus("disconnected")
      console.error("Failed to connect agent:", err)
    }
  }, [])

  const disconnect = useCallback(() => {
    cleanupRef.current?.()
    tauriApi.ptyKill().catch(() => {})
    setStatus("disconnected")
  }, [])

  useEffect(() => {
    return () => {
      cleanupRef.current?.()
    }
  }, [])

  const writeToTerminal = useCallback((data: string) => {
    tauriApi.ptyWrite(data).catch(() => {})
  }, [])

  const resizeTerminal = useCallback((cols: number, rows: number) => {
    tauriApi.ptyResize(cols, rows).catch(() => {})
  }, [])

  const openFile = useCallback(async (path: string) => {
    try {
      const content = await tauriApi.readFile(path)
      setFileContents((prev) => ({ ...prev, [path]: content }))
      return content
    } catch {
      return null
    }
  }, [])

  const createFile_ = useCallback(async (path: string, content: string) => {
    await tauriApi.createFile(path, content)
  }, [])

  const changeDir = useCallback(async (path: string) => {
    const tree = await tauriApi.changeWorkDir(path)
    setFileTree(tree)
    setWorkDir(path)
  }, [])

  const refreshFileTree = useCallback(async () => {
    try {
      const tree = await tauriApi.listFiles()
      setFileTree(tree)
    } catch {
      /* ignore */
    }
  }, [])

  const onTerminalData = useCallback((callback: (data: string) => void) => {
    terminalCallbackRef.current = callback
    return () => {
      terminalCallbackRef.current = null
    }
  }, [])

  return {
    status,
    fileTree,
    fileContents,
    workDir,
    terminalOutput,
    connect,
    disconnect,
    writeToTerminal,
    resizeTerminal,
    openFile,
    createFile: createFile_,
    changeDir,
    refreshFileTree,
    onTerminalData,
  }
}
```

- [ ] **Step 2: Commit**

```bash
git add hooks/use-tauri-agent.ts
git commit -m "feat: add useTauriAgent hook replacing agent WebSocket (Phase 8c)"
```

---

### Task 10: Update code-canvas.tsx to use Tauri agent when available

**Files:**
- Modify: `components/dashboard/code-canvas.tsx`

- [ ] **Step 1: Replace agent WebSocket logic with Tauri API**

This is the core integration task. The `code-canvas.tsx` component needs to detect whether it's running in Tauri and use the Tauri agent hook instead of WebSocket.

Add these imports at the top of code-canvas.tsx (after existing imports):

```typescript
import { useTauriAgent } from "@/hooks/use-tauri-agent"
import * as tauriApi from "@/lib/tauri-api"

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
}
```

Then, in the `CodeCanvas` component, add the Tauri agent hook alongside the existing WebSocket logic. The key change: when in Tauri, use `useTauriAgent` instead of the WebSocket connection.

Add after the existing `useEffect` for the WebSocket agent connection (around line 543):

```typescript
// ─── Tauri Agent (replaces WebSocket when running in Tauri) ───
const tauriAgent = useTauriAgent()
const isTauriMode = useRef(isTauri())

useEffect(() => {
  if (!isTauriMode.current) return
  tauriAgent.connect()
  return () => {
    tauriAgent.disconnect()
  }
}, [])

// Feed terminal output from Tauri agent to xterm
useEffect(() => {
  if (!isTauriMode.current) return
  return tauriAgent.onTerminalData((data) => {
    writeTerminal(data)
  })
}, [tauriAgent])

// Sync Tauri agent file tree
useEffect(() => {
  if (!isTauriMode.current || !tauriAgent.fileTree) return
  setAgentFileTree(tauriAgent.fileTree)
}, [tauriAgent.fileTree])

// Sync Tauri agent file contents
useEffect(() => {
  if (!isTauriMode.current) return
  setAgentFileContents(tauriAgent.fileContents)
}, [tauriAgent.fileContents])

// Sync Tauri agent status
useEffect(() => {
  if (!isTauriMode.current) return
  setAgentStatus(tauriAgent.status)
}, [tauriAgent.status])

// Sync Tauri agent work dir
useEffect(() => {
  if (!isTauriMode.current || !tauriAgent.workDir) return
  setWorkDir(tauriAgent.workDir)
}, [tauriAgent.workDir])
```

Then modify the `sendAgentMessage` function to route through Tauri when available:

```typescript
const sendAgentMessage = useCallback((message: unknown) => {
  if (isTauriMode.current) {
    const msg = message as { type: string; data?: string; path?: string; content?: string; cols?: number; rows?: number }
    switch (msg.type) {
      case "terminal_input":
        if (msg.data) tauriAgent.writeToTerminal(msg.data)
        break
      case "terminal_resize":
        if (msg.cols && msg.rows) tauriAgent.resizeTerminal(msg.cols, msg.rows)
        break
      case "open_file":
        if (msg.path) tauriAgent.openFile(msg.path)
        break
      case "list_files":
        tauriAgent.refreshFileTree()
        break
      case "change_work_dir":
        if (msg.path) tauriAgent.changeDir(msg.path)
        break
      case "create_file":
        if (msg.path && msg.content) tauriAgent.createFile(msg.path, msg.content)
        break
      case "new_file":
      case "new_folder":
        writeTerminal("\r\n[agent] New File/Folder: use the terminal\r\n")
        break
    }
    return
  }
  // Original WebSocket path
  if (agentWsRef.current?.readyState === WebSocket.OPEN) {
    agentWsRef.current.send(JSON.stringify(message))
    return true
  }
  writeTerminal("\r\n[agent] local agent websocket is not connected\r\n")
  return false
}, [tauriAgent])
```

- [ ] **Step 2: Commit**

```bash
git add components/dashboard/code-canvas.tsx
git commit -m "feat: integrate Tauri agent API into CodeCanvas (Phase 8c)"
```

---

### Task 11: Add @tauri-apps/api as a required dependency

**Files:**
- Modify: `package.json`

- [ ] **Step 1: Move @tauri-apps/api from optionalDependencies to dependencies**

Run:
```bash
cd D:/SASU/AI/mathmodel && npm install @tauri-apps/api@^2.11.0 --save
```

- [ ] **Step 2: Commit**

```bash
git add package.json package-lock.json
git commit -m "chore: move @tauri-apps/api to production dependencies"
```

---

### Task 12: Build verification

**Files:**
- Verify: `src-tauri/` compiles
- Verify: `npm run build` succeeds

- [ ] **Step 1: Verify Tauri Rust compilation**

Run: `cd src-tauri && cargo check 2>&1`
Expected: No errors.

- [ ] **Step 2: Verify Next.js build**

Run: `npm run build`
Expected: Build succeeds with static export to `out/`.

- [ ] **Step 3: Commit any build fixes**

If build errors occurred, fix them and commit.

---

## Phase 8d — Frontend Server API Migration (Outlined)

After Phase 8c is complete and working, Phase 8d wraps the remaining server HTTP calls in `lib/api.ts` with Tauri invoke. This is a bridge phase — the Tauri commands will internally proxy to the server via HTTP (until Phase 8e embeds it).

Key tasks:
1. Create `src-tauri/src/server_bridge/mod.rs` with Tauri commands that call the server via `reqwest` on localhost
2. Add `reqwest` to `src-tauri/Cargo.toml`
3. Create typed wrappers in `lib/tauri-api.ts` for each server API endpoint
4. Modify `lib/api.ts` to use Tauri invoke when available, falling back to `fetch`
5. Handle auth token sharing between frontend and Tauri commands

## Phase 8e — Server Embedding (Outlined)

The final phase embeds the Axum server into the Tauri process:

1. Add `modeler-server = { path = "../server" }` to `src-tauri/Cargo.toml`
2. Initialize DB pool, room registry, agent registry in Tauri `setup()`
3. Start Axum server on `127.0.0.1:0` (random port) in background tokio task
4. Pass the port to frontend via Tauri event
5. Replace `src-tauri/src/server_bridge/` proxy commands with direct function calls into server handlers
6. Adjust server's `config.rs` to use Tauri's app data directory for DB and file storage

---

## Self-Review

**1. Spec coverage:** All four phases (8b, 8c, 8d, 8e) are covered. 8b and 8c have full task-level detail. 8d and 8e are outlined — detailed tasks will be written after 8c is complete and working.

**2. Placeholder scan:** No TBD/TODO/fill-in-the-blanks. All code blocks are complete and ready to implement.

**3. Type consistency:** Event variant names match between Rust `AgentEvent` enum and TypeScript interfaces. Command function names match between Rust `#[tauri::command]` functions and TypeScript `invoke()` calls. `FileTreeItem` has identical shape in both Rust and TypeScript.
