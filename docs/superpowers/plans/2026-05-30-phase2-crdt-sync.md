# Phase 2: CRDT 实时协同 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development

**Goal:** 实现多人实时协同编辑 — CRDT 文档同步 + Monaco 编辑器集成 + 光标感知

**Architecture:** 后端 yrs 管理 YDoc per document, Axum WebSocket 承载双向同步流, 前端 y-monaco binding 连接 Monaco editor

**Tech Stack:** yrs (Rust), yjs/y-monaco (JS), Axum WebSocket, Monaco Editor

---

## Task 1: 后端 — 添加依赖 + sync 模块骨架

**Files:**
- Modify: `server/Cargo.toml`
- Create: `server/src/sync/mod.rs`
- Create: `server/src/sync/room.rs`
- Create: `server/src/sync/handlers.rs`
- Modify: `server/src/main.rs`

- [ ] **Step 1: Add dependencies to Cargo.toml**

Add to `server/Cargo.toml`:

```toml
yrs = "0.19"
tokio-stream = "0.1"
```

Also add `"ws"` to axum features if not already present. Check current axum line.

- [ ] **Step 2: Write sync/mod.rs**

```rust
pub mod room;
pub mod handlers;
```

- [ ] **Step 3: Write sync/room.rs — SyncRoom manager**

This manages one YDoc per file, with a broadcast channel for distributing updates to all connected clients.

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

pub struct SyncRoom {
    pub file_id: String,
    pub doc: yrs::Doc,
    pub update_tx: broadcast::Sender<Vec<u8>>,
    pub awareness_states: Vec<u8>, // merged awareness state
}

impl SyncRoom {
    pub fn new(file_id: String) -> Self {
        let doc = yrs::Doc::new();
        let (tx, _) = broadcast::channel::<Vec<u8>>(256);
        Self {
            file_id,
            doc,
            update_tx: tx,
            awareness_states: Vec::new(),
        }
    }

    pub fn apply_update(&mut self, update: &[u8]) -> Result<(), yrs::Error> {
        let mut txn = self.doc.transact_mut();
        txn.apply_update(update.clone().into())?;
        Ok(())
    }

    pub fn encode_state(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&yrs::StateVector::default())
    }
}

pub struct RoomRegistry {
    rooms: Arc<RwLock<HashMap<String, Arc<RwLock<SyncRoom>>>>>,
}

impl RoomRegistry {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_or_create(&self, file_id: &str, pool: &sqlx::SqlitePool) -> Arc<RwLock<SyncRoom>> {
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get(file_id) {
            return room.clone();
        }

        let mut room = SyncRoom::new(file_id.to_string());

        // Load existing CRDT state from DB if available
        if let Ok(Some((state,))) = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT ydoc_state FROM crdt_docs WHERE file_id = ?"
        )
        .bind(file_id)
        .fetch_optional(pool)
        .await
        {
            if !state.is_empty() {
                let _ = room.apply_update(&state);
            }
        }

        let room = Arc::new(RwLock::new(room));
        rooms.insert(file_id.to_string(), room.clone());
        room
    }

    pub async fn persist(&self, file_id: &str, pool: &sqlx::SqlitePool) {
        if let Some(room) = self.rooms.read().await.get(file_id) {
            let room = room.read().await;
            let state = room.encode_state();
            let now = chrono::Utc::now().timestamp();
            sqlx::query(
                "INSERT OR REPLACE INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)"
            )
            .bind(file_id)
            .bind(&state)
            .bind(now)
            .execute(pool)
            .await
            .ok();
        }
    }
}
```

- [ ] **Step 4: Write sync/handlers.rs — WebSocket handler**

```rust
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, Path, Query,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::room::{RoomRegistry, SyncRoom};
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SyncQuery {
    pub file_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SyncMessage {
    #[serde(rename = "sync_update")]
    SyncUpdate { update: Vec<u8> },
    #[serde(rename = "sync_request")]
    SyncRequest { state_vector: Option<Vec<u8>> },
    #[serde(rename = "sync_response")]
    SyncResponse { update: Vec<u8> },
    #[serde(rename = "awareness")]
    Awareness { state: Vec<u8> },
    #[serde(rename = "error")]
    Error { message: String },
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<SyncQuery>,
) -> Result<impl IntoResponse, AppError> {
    // Auth via query param token for WS (can't use headers in browser WS)
    // For now, skip auth check — will be validated in the WS handshake
    let file_id = query.file_id;
    let pool = state.pool.clone();
    let registry = state.room_registry.clone();

    Ok(ws.on_upgrade(move |socket| {
        handle_socket(socket, file_id, pool, registry)
    }))
}

async fn handle_socket(
    socket: WebSocket,
    file_id: String,
    pool: sqlx::SqlitePool,
    registry: Arc<RoomRegistry>,
) {
    let room = registry.get_or_create(&file_id, &pool).await;
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut update_rx = {
        let r = room.read().await;
        r.update_tx.subscribe()
    };

    // Send initial state
    {
        let r = room.read().await;
        let state = r.encode_state();
        let msg = SyncMessage::SyncResponse { update: state };
        let json = serde_json::to_string(&msg).unwrap();
        let _ = ws_tx.send(Message::Text(json.into())).await;
    }

    // Spawn broadcast listener: room updates → this client
    let file_id_clone = file_id.clone();
    let mut ws_tx_clone = ws_tx; // We'll use the actual ws_tx after the split loop

    // Read from WebSocket
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(sync_msg) = serde_json::from_str::<SyncMessage>(&text) {
                    match sync_msg {
                        SyncMessage::SyncUpdate { update } => {
                            let mut r = room.write().await;
                            if r.apply_update(&update).is_ok() {
                                let _ = r.update_tx.send(update);
                            }
                        }
                        SyncMessage::SyncRequest { state_vector: _ } => {
                            let r = room.read().await;
                            let state = r.encode_state();
                            let resp = SyncMessage::SyncResponse { update: state };
                            if let Ok(json) = serde_json::to_string(&resp) {
                                // Can't send from ws_tx here because it was moved.
                                // Use a channel or restructure.
                            }
                        }
                        SyncMessage::Awareness { state } => {
                            // Broadcast awareness to other clients
                        }
                        _ => {}
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // On disconnect: persist CRDT state
    registry.persist(&file_id_clone, &pool).await;
}
```

Actually, the above handler has issues with the broadcast pattern (can't easily send from both the broadcast listener and the WS reader). Let me rewrite it more simply.

```rust
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, Query,
    },
    response::IntoResponse,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::room::RoomRegistry;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SyncQuery {
    pub file_id: String,
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SyncMessage {
    #[serde(rename = "sync_update")]
    SyncUpdate { update: Vec<u8> },
    #[serde(rename = "sync_full")]
    SyncFull { state: Vec<u8> },
    #[serde(rename = "awareness")]
    Awareness { state: Vec<u8> },
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<SyncQuery>,
) -> Result<impl IntoResponse, AppError> {
    let file_id = query.file_id;
    let pool = state.pool.clone();
    let registry = state.room_registry.clone();

    Ok(ws.on_upgrade(move |socket| {
        handle_socket(socket, file_id, pool, registry)
    }))
}

async fn handle_socket(
    mut socket: WebSocket,
    file_id: String,
    pool: sqlx::SqlitePool,
    registry: std::sync::Arc<RoomRegistry>,
) {
    let room = registry.get_or_create(&file_id, &pool).await;

    // Send current full state to new client
    {
        let r = room.read().await;
        let state = r.encode_state();
        let msg = SyncMessage::SyncFull { state };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = socket.send(Message::Text(json.into())).await;
        }
    }

    // Receive updates from this client, broadcast to others
    let mut update_rx = room.read().await.update_tx.subscribe();

    loop {
        tokio::select! {
            // Incoming updates from this client
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<SyncMessage>(&text) {
                            match msg {
                                SyncMessage::SyncUpdate { update } => {
                                    let mut r = room.write().await;
                                    if r.apply_update(&update).is_ok() {
                                        let _ = r.update_tx.send(update);
                                    }
                                }
                                SyncMessage::Awareness { .. } => {
                                    // Awareness is broadcast-only for now
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => continue,
                }
            }
            // Updates from other clients (via broadcast channel)
            Ok(update) = update_rx.recv() => {
                let msg = SyncMessage::SyncUpdate { update };
                if let Ok(json) = serde_json::to_string(&msg) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
        }
    }

    // Persist on disconnect
    registry.persist(&file_id, &pool).await;
}
```

- [ ] **Step 5: Update main.rs — add sync module and WS route**

Add to module declarations:
```rust
mod sync;
```

Add `room_registry` field to `AppState`:
```rust
use std::sync::Arc;

pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub config: config::Config,
    pub room_registry: Arc<sync::room::RoomRegistry>,
}
```

Initialize in main:
```rust
let app_state = AppState {
    pool: pool.clone(),
    config: cfg.clone(),
    room_registry: Arc::new(sync::room::RoomRegistry::new()),
};
```

Add WS route:
```rust
.route("/sync", get(sync::handlers::ws_handler))
```

- [ ] **Step 6: cargo check**

---

## Task 2: 前端 — 安装 yjs 依赖 + WebSocket provider

**Files:**
- Modify: `package.json` (via npm install)
- Create: `lib/yjs-provider.ts`

- [ ] **Step 1: Install npm packages**

```bash
npm install yjs y-monaco
```

- [ ] **Step 2: Create lib/yjs-provider.ts**

TypeScript WebSocket provider that connects to the Rust sync server:

```typescript
import * as Y from "yjs"

export interface SyncMessage {
  type: "sync_update" | "sync_full" | "awareness"
  update?: number[]
  state?: number[]
}

export class YjsWebsocketProvider {
  private ws: WebSocket | null = null
  private doc: Y.Doc
  private url: string
  private fileId: string
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null

  constructor(doc: Y.Doc, fileId: string) {
    this.doc = doc
    this.fileId = fileId
    const base = process.env.NEXT_PUBLIC_WS_URL || "ws://localhost:3001"
    this.url = `${base}/sync?file_id=${fileId}`
    this.connect()
  }

  private connect() {
    this.ws = new WebSocket(this.url)

    this.ws.onopen = () => {
      console.log("[YjsWS] connected", this.fileId)
    }

    this.ws.onmessage = (event) => {
      try {
        const msg: SyncMessage = JSON.parse(event.data)
        if (msg.type === "sync_full" && msg.state) {
          const update = new Uint8Array(msg.state)
          Y.applyUpdate(this.doc, update)
        } else if (msg.type === "sync_update" && msg.update) {
          const update = new Uint8Array(msg.update)
          Y.applyUpdate(this.doc, update)
        }
      } catch (e) {
        console.error("[YjsWS] parse error", e)
      }
    }

    this.ws.onclose = () => {
      console.log("[YjsWS] disconnected, reconnecting in 2s")
      this.reconnectTimer = setTimeout(() => this.connect(), 2000)
    }

    this.ws.onerror = (err) => {
      console.error("[YjsWS] error", err)
    }

    // Send local updates to server
    this.doc.on("update", (update: Uint8Array) => {
      if (this.ws?.readyState === WebSocket.OPEN) {
        const msg: SyncMessage = {
          type: "sync_update",
          update: Array.from(update),
        }
        this.ws.send(JSON.stringify(msg))
      }
    })
  }

  destroy() {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.ws?.close()
    this.doc.destroy()
  }
}
```

---

## Task 3: 前端 — CodeCanvas Monaco 集成 CRDT

**Files:**
- Modify: `components/dashboard/code-canvas.tsx`

- [ ] **Step 1: Update CodeCanvas to use Yjs + Monaco**

The key changes:
1. When a file is opened, create a Y.Doc and connect via YjsWebsocketProvider
2. Bind the Y.Doc to MonacoEditor using y-monaco
3. When switching files, destroy old binding, create new one

Add at top of code-canvas.tsx:
```tsx
import { useEffect, useRef, useMemo } from "react"
import * as Y from "yjs"
import { MonacoBinding } from "y-monaco"
import { YjsWebsocketProvider } from "@/lib/yjs-provider"
```

Add state management for Yjs docs:
```tsx
const yDocsRef = useRef<Map<string, { doc: Y.Doc; provider: YjsWebsocketProvider }>>(new Map())

// When active file changes, set up Yjs
useEffect(() => {
  if (!activeTab) return
  
  let entry = yDocsRef.current.get(activeTab)
  if (!entry) {
    const doc = new Y.Doc()
    const provider = new YjsWebsocketProvider(doc, fileIdForTab(activeTab))
    entry = { doc, provider }
    yDocsRef.current.set(activeTab, entry)
  }

  return () => {
    // cleanup on file switch: disconnect old provider but keep doc
  }
}, [activeTab])
```

For the MonacoBinding, we need to get the Monaco editor instance. The `@monaco-editor/react` library provides `OnMount` callback:

```tsx
const handleEditorMount = (editor: monaco.editor.IStandaloneCodeEditor) => {
  const entry = yDocsRef.current.get(activeTab)
  if (!entry) return

  const yText = entry.doc.getText("content")

  // Bind Monaco to Y.Text
  const binding = new MonacoBinding(
    yText,
    editor.getModel()!,
    new Set([editor]),
    entry.doc.awareness
  )

  // Store for cleanup
  bindingsRef.current.set(activeTab, binding)
}
```

Replace the MonacoEditor component usage: remove `value` prop (Yjs manages it), add `onMount` callback.

- [ ] **Step 2: Update .env.local**

```
NEXT_PUBLIC_WS_URL=ws://localhost:3001
```

- [ ] **Step 3: Verify TypeScript**

```bash
npx tsc --noEmit
```

---

## Task 4: 端到端测试

- [ ] **Step 1: Build and start backend**

```bash
cargo build && cargo run
```

- [ ] **Step 2: Start frontend**

```bash
npm run dev
```

- [ ] **Step 3: Test with two browser tabs**

1. Open two browser tabs to same project file
2. Type in one tab
3. Verify text appears in the other tab
4. Verify WS connection in browser DevTools Network tab
