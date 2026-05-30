# Modeler AI 全栈设计文档

> 日期：2026-05-29 | 状态：待审核

## 一、产品定位与场景

**数学建模竞赛协作平台** — 面向 MCM/ICM 等 3-4 人团队，在 96 小时竞赛窗口内完成建模、编程、论文写作的全流程协作工具。

核心原则：
- 模块化单体架构，不过度拆分
- 单二进制部署，零运维负担
- 实时协同为第一优先级

---

## 二、技术栈

| 层 | 选型 | 理由 |
|---|---|---|
| 前端 | Next.js 16 + React 19 + TypeScript | 保持现有 |
| UI 组件 | shadcn/ui (Radix) + Tailwind CSS 4 | 保持现有 |
| 代码编辑器 | Monaco Editor | 保持现有 |
| 后端框架 | **Axum** (tokio) | Rust 异步框架标杆 |
| 实时协同 | **yrs** (Yjs Rust) + Axum WebSocket | 最佳 CRDT 实现 |
| 数据库 | **SQLite** via **SQLx** | 编译期检查、零配置、单文件 |
| 认证 | JWT (jsonwebtoken) | 无状态、扩展简单 |
| AI 中转 | reqwest + tower-rate-limit | 类 one-api 的 HTTP 代理 |
| 代码执行 | **Docker** (bollard crate) | 沙箱隔离 |
| 本地 Agent | Rust 轻量进程 | WebSocket 桥接 Claude Code / Tabbit |
| 部署 | 单一二进制 + SQLite 文件 | 无需 docker compose |

---

## 三、完整架构图

```
┌──────────────────────────────────────────────────────────────────┐
│                        Next.js 前端                              │
│                                                                  │
│  ┌───────────┐  ┌──────────────────┐  ┌─────────────────────┐  │
│  │  Sidebar  │  │  MainWorkspace   │  │     CodeCanvas       │  │
│  │           │  │                  │  │                     │  │
│  │ - 模块导航 │  │ - AI文献搜索     │  │ - Monaco 编辑器     │  │
│  │ - 代码区  │  │ - 论文编辑区     │  │ - Terminal (agent)  │  │
│  │ - 文档区  │  │ - 资料收集板     │  │ - 输出图表          │  │
│  │ - 资料区  │  │ - AI 对话面板    │  │ - 协同光标/状态     │  │
│  └───────────┘  └──────────────────┘  └─────────────────────┘  │
│       │                │                       │                │
│       └────────────────┼───────────────────────┘                │
│                REST API + WebSocket (CRDT + Terminal)           │
└────────────────────────┼──────────────────────────────────────────┘
                         │
┌────────────────────────▼──────────────────────────────────────────┐
│                    Rust Backend (Axum 单进程)                      │
│                                                                    │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐    │
│  │  auth/   │ │  sync/   │ │  file/   │ │  ai/             │    │
│  │          │ │          │ │          │ │                  │    │
│  │ JWT签发  │ │ yrs CRDT │ │ 上传下载  │ │ 多厂商路由       │    │
│  │ + 邀请码  │ │ WS连接池 │ │ 文件夹管理│ │ API Key 池管理   │    │
│  │          │ │ awareness│ │ 二进制存储│ │ 额度/速率控制    │    │
│  └──────────┘ └──────────┘ └──────────┘ │ 日志审计         │    │
│                                         └──────────────────┘    │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐    │
│  │ compute/ │ │ history/ │ │ project/ │ │agent-bridge/     │    │
│  │          │ │          │ │          │ │                  │    │
│  │ Docker   │ │ 自动快照  │ │ CRUD     │ │ WS通道管理       │    │
│  │ 沙箱执行  │ │ 时间线API │ │ 权限模型  │ │ Claude Code指令  │    │
│  │ pip/apt  │ │ 回退接口  │ │ 角色管理  │ │ Tabbit 数据接收  │    │
│  └──────────┘ └──────────┘ └──────────┘ └──────────────────┘    │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │              SQLite (SQLx) — 单文件                       │    │
│  │   users | projects | documents | files | api_keys |      │    │
│  │   history_snapshots | compute_sessions                   │    │
│  └──────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│              Local Agent (Rust) — 每队员电脑一个                  │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │  WS Client   │  │  Claude Code │  │   Tabbit Bridge      │   │
│  │  (连平台)     │  │  进程管理     │  │   本地 HTTP 接收     │   │
│  └──────────────┘  └──────────────┘  └──────────────────────┘   │
│  ┌──────────────┐  ┌──────────────┐                             │
│  │  File Watch  │  │  PTY Manager │                             │
│  │  (notify)    │  │  (终端透传)   │                             │
│  └──────────────┘  └──────────────┘                             │
└──────────────────────────────────────────────────────────────────┘

外部服务：
  Tavily API ── 文献搜索
  Claude API ── 写作/代码
  OpenAI API ── 通用
  多模型 API ── DeepSeek / Kimi / Qwen / GLM（通过 AI 中转站统一路由）
```

---

## 四、模块详细设计

### 4.1 auth — 认证模块

**职责**：用户注册/登录 + 项目邀请 + JWT 管理

**数据模型**：
- `users(id, email, password_hash, display_name, created_at)`
- `project_members(project_id, user_id, role, joined_at)`
- `invite_codes(id, project_id, code, max_uses, used_count, expires_at)`

**接口**：
- `POST /auth/register` — 邮箱注册
- `POST /auth/login` — 登录，返回 JWT
- `POST /auth/refresh` — 刷新 token
- `POST /projects/{id}/invite` — 生成邀请码
- `POST /projects/join` — 通过邀请码加入

**认证流**：Bearer JWT in Authorization header，过期 24h，refresh token 7d。

---

### 4.2 sync — 实时协同模块

**职责**：基于 CRDT 的多人实时编辑 + 在线状态感知

**技术方案**：
- **yrs** (Yjs Rust) 作为 CRDT 引擎
- 每个打开的文档对应一个 `YDoc`
- Axum WebSocket 承载双向同步流
- Awareness protocol：谁在线、光标位置、选中范围

**连接模型**：
```
Client A ──WS──┐
               ├── SyncRoom (per document) ── yrs YDoc
Client B ──WS──┘
```

**同步流程**：
1. Client 打开文件 → WS 连接 → 请求 doc state vector
2. Server 对比 state vector，发送 delta（SyncStep1）
3. Client 收到 delta 后 apply → 本地 YDoc 更新 → Monaco 渲染
4. Client 本地编辑 → 产生 YUpdate → 通过 WS 发送给 server
5. Server 广播 YUpdate 给同文档其他连接

**协同范围**：
- 代码文件（.py, .m, .jl 等）：CRDT 字符级同步
- 文档文件（.tex, .md）：CRDT 字符级同步
- 资料文件（PDF, 数据文件）：仅上传/下载，不可实时编辑

**光标感知** (Awareness)：
- 每个连接的 user_id + display_name + cursor_position
- 前端 Monaco Editor 显示队友光标（不同颜色）
- 侧边栏显示在线状态

---

### 4.3 file — 文件管理模块

**职责**：CRUD 文件/文件夹 + 存储

**数据模型**：
- `files(id, project_id, parent_id, name, type={file|folder}, mime_type, size, storage_path, created_at, updated_at)`
- `file_contents(id, file_id, content)` — 仅用于非 CRDT 文件（二进制等）

**接口**：
- `GET /projects/{id}/files?path=` — 列出目录内容
- `POST /projects/{id}/files` — 上传文件 (multipart)
- `GET /projects/{id}/files/{id}/download` — 下载
- `DELETE /projects/{id}/files/{id}` — 删除
- `PUT /projects/{id}/files/{id}/rename` — 重命名/移动

**存储**：小文件存 SQLite，大文件（>1MB）存本地文件系统 `data/<project_id>/`。

---

### 4.4 ai — AI 中转站模块

**职责**：多厂商统一接口、API Key 管理、额度控制、审计日志

**设计参考**：one-api 的接口规范，Rust 重新实现

**路由表** (`api_routes` 表)：
```
POST /ai/v1/chat/completions
  → 根据 model 前缀路由：
    claude-*     → Claude API
    gpt-*/o1-*  → OpenAI API  
    deepseek-*  → DeepSeek API
    kimi-*      → Moonshot API
    qwen-*      → Tongyi API
    glm-*       → Zhipu API
    search      → Tavily Search API
    local       → 本地 Claude Code (via Agent)
```

**功能**：
- **Key 池管理**：管理员添加多个 API Key，系统按权重/轮询分配
- **额度控制**：每个项目设置 token 上限，达到后拒绝请求
- **速率限制**：per-user per-minute 限制（tower-rate-limit）
- **日志审计**：每次请求记录 user/project/model/tokens/cost
- **统一响应格式**：兼容 OpenAI chat completion 格式

**接口**：
- `POST /ai/v1/chat/completions` — 统一聊天入口
- `POST /ai/v1/search` — 文献搜索（Tavily）
- `GET /ai/v1/models` — 列出可用模型及额度
- `POST /admin/keys` — 管理 API Key（管理员）

---

### 4.5 compute — 计算执行模块

**职责**：Docker 沙箱执行模型代码，持久化环境

**架构**：
```
每个 project 对应一个 Docker volume (持久化 pip/apt 安装)
基础镜像: python:3.11-slim + numpy/scipy/matplotlib/pandas 预装
项目专属层: pip install tensorflow ... (在 volume 里)
```

**执行流**：
1. 前端提交代码 + 语言类型（Python / Octave / Julia）
2. Server 创建容器（挂载项目 volume + 代码文件）
3. 执行代码 → 捕获 stdout/stderr
4. 返回输出文本 + 图表（base64 png）+ 退出码
5. 容器销毁（volume 保留）

**接口**：
- `POST /compute/run` — 执行代码片段，返回输出
- `POST /compute/terminal` — WebSocket，实时终端连接容器 bash
- `POST /compute/reset` — 重置项目环境到基础镜像
- `GET /compute/packages` — 列出已安装的包

**安全**：
- 容器无网络访问（除 pip 安装时临时开放）
- CPU/内存限制（cgroups）
- 超时 300 秒自动 kill
- seccomp profile 限制系统调用

---

### 4.6 history — 版本历史模块

**职责**：自动快照 + 手动保存点 + 时间线回退

**数据模型**：
- `snapshots(id, project_id, path, content, created_at, created_by, label)`

**快照策略**：
- 自动：文件关闭/切换时保存
- 手动：用户点击 "Save Checkpoint"
- 保留最近 100 个快照，超过则合并早期快照

**接口**：
- `GET /projects/{id}/history?path=` — 获取某文件的时间线
- `POST /projects/{id}/history/checkpoint` — 手动创建保存点
- `POST /projects/{id}/history/restore/{snapshot_id}` — 回退到指定快照
- `GET /projects/{id}/history/diff/{from}/{to}` — 对比两个快照

**UI 呈现**：时间线滑块，拖拽预览历史版本，点击恢复。

---

### 4.7 agent-bridge — 本地 Agent 桥接

**职责**：连接队员本地 Rust Agent，透传 Claude Code 交互

**平台端接口**：
- WS `wss://<server>/agent` — 长连接（需 JWT 认证）
- 消息格式：JSON Frame
  ```json
  {
    "type": "terminal_input" | "terminal_output" | 
            "file_change" | "claude_command" | "tabbit_data",
    "project_id": "...",
    "payload": { ... }
  }
  ```

**Local Agent 职责**：
1. **WS Client** — 初始化时连平台，维持心跳
2. **Claude Code 进程管理** — spawn `claude` CLI，通过 PTY 捕获输入输出
3. **PTY 透传** — 前端 Terminal 组件 → WS → PTY stdin → claude CLI → PTY stdout → WS → 前端渲染
4. **文件变更监听** — notify crate 监听本地工作目录 → 文件变更自动同步到平台 CRDT
5. **Tabbit HTTP Bridge** — 监听 `localhost:PORT`，接收 Tabbit 推送的文献/笔记

**Claude Code 交互流程**：
```
用户在 CodeCanvas Terminal 输入 "claude run 写一个SIR模型"
  → WS 发到 Local Agent
  → Agent PTY stdin 写入
  → Claude CLI 执行
  → PTY stdout 流式返回
  → Agent WS 推回前端 Terminal 渲染
  → 文件变更由 Agent 监听到
  → Agent 通过 WS 同步文件内容到平台 Sync 模块
  → 队友的编辑器实时看到新代码
```

**Tabbit 集成**：
- Tabbit "妙招" 脚本配置为 `POST http://localhost:<agent_port>/tabbit`
- 发送内容：{url, title, summary, notes, context_pages}
- Local Agent 接收后转发到平台 Research 区存储
- 全队可见，可标注、引用到论文

---

### 4.8 project — 项目管理模块

**职责**：项目 CRUD + 权限 + 角色

**工作空间结构**：
```
Project (一个比赛)
├── 📁 Code (代码区)     — CRDT 协同编辑，默认全员可写
├── 📁 Paper (文档区)    — CRDT 协同编辑，默认全员可写
└── 📁 Research (资料区) — 文件管理 + 笔记，全员可读，可设编辑权限
```

**权限模型**：
- `owner` — 创建者，可管理成员/权限/删除项目
- `editor` — 可编辑所有文件
- `viewer` — 只读

**接口**：
- `POST /projects` — 创建项目
- `GET /projects/{id}` — 项目详情
- `PUT /projects/{id}/permissions` — 设置子区权限
- `GET /projects/{id}/members` — 成员列表
- `DELETE /projects/{id}/members/{user_id}` — 移除成员

---

## 五、数据库 Schema

```sql
-- 用户
CREATE TABLE users (
    id TEXT PRIMARY KEY,  -- UUID
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at INTEGER NOT NULL  -- unix timestamp
);

-- 项目
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    owner_id TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- 项目成员
CREATE TABLE project_members (
    project_id TEXT NOT NULL REFERENCES projects(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    role TEXT NOT NULL DEFAULT 'editor',  -- owner|editor|viewer
    joined_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, user_id)
);

-- 邀请码
CREATE TABLE invite_codes (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    code TEXT UNIQUE NOT NULL,
    max_uses INTEGER DEFAULT 10,
    used_count INTEGER DEFAULT 0,
    expires_at INTEGER,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL
);

-- 文件元数据
CREATE TABLE files (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    parent_id TEXT REFERENCES files(id),
    name TEXT NOT NULL,
    type TEXT NOT NULL,  -- file|folder
    mime_type TEXT,
    size INTEGER DEFAULT 0,
    storage_path TEXT,   -- null for folders
    zone TEXT NOT NULL DEFAULT 'code',  -- code|paper|research
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, parent_id, name)
);

-- CRDT 文档状态 (yrs binary)
CREATE TABLE crdt_docs (
    file_id TEXT PRIMARY KEY REFERENCES files(id),
    ydoc_state BLOB NOT NULL,  -- Yrs encoded state vector
    updated_at INTEGER NOT NULL
);

-- 非文本文件内容
CREATE TABLE file_blobs (
    file_id TEXT PRIMARY KEY REFERENCES files(id),
    content BLOB NOT NULL
);

-- 历史快照
CREATE TABLE snapshots (
    id TEXT PRIMARY KEY,
    file_id TEXT NOT NULL REFERENCES files(id),
    project_id TEXT NOT NULL REFERENCES projects(id),
    label TEXT,
    ydoc_state BLOB NOT NULL,  -- CRDT full state at snapshot
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL
);

-- API Key 池
CREATE TABLE api_keys (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,  -- openai|claude|deepseek|kimi|qwen|glm|tavily
    key_encrypted TEXT NOT NULL,
    weight INTEGER DEFAULT 1,
    is_active INTEGER DEFAULT 1,
    created_at INTEGER NOT NULL
);

-- AI 请求日志
CREATE TABLE ai_usage_logs (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    project_id TEXT REFERENCES projects(id),
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt_tokens INTEGER DEFAULT 0,
    completion_tokens INTEGER DEFAULT 0,
    cost REAL DEFAULT 0.0,
    created_at INTEGER NOT NULL
);

-- 项目 token 额度
CREATE TABLE project_quotas (
    project_id TEXT PRIMARY KEY REFERENCES projects(id),
    total_tokens_used INTEGER DEFAULT 0,
    token_limit INTEGER DEFAULT 100000000  -- 100M tokens
);

-- 计算会话
CREATE TABLE compute_sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    language TEXT NOT NULL,
    status TEXT NOT NULL,  -- running|completed|error
    stdout TEXT,
    stderr TEXT,
    exit_code INTEGER,
    duration_ms INTEGER,
    created_at INTEGER NOT NULL
);
```

---

## 六、前端调整

### 6.1 原三栏布局不变，功能增强

```
┌──────────────┬───────────────────────┬─────────────────────────┐
│   Sidebar    │   MainWorkspace      │     CodeCanvas          │
│              │                      │                         │
│ ┌模块导航────┐│ ① AI Search          │ ┌─在线队友列表──────────┐│
│ │ 📁 Code   ││   (Tavily + 中转站)  │ │ Alice 🟢 coding     ││
│ │ 📄 Paper  ││                      │ │ Bob   🟢 writing     ││
│ │ 🔬 Research││ ② Paper Editor       │ │ Carol 🟡 idle        ││
│ │           ││   (LaTeX/MD CRDT)    │ │                      ││
│ │           ││                      │ └──────────────────────┘│
│ │ 👥 Team   ││ ③ Research Board     │                         │
│ │ 🔑 Keys   ││   (文献卡片 + 笔记)   │ Monaco Editor ────┐     │
│ │           ││                      │  (CRDT 同步)      │     │
│ │           ││ ④ AI Chat Panel      │                    │     │
│ │           ││   (对话辅助)          │ Terminal ──────┐  │     │
│ └───────────┘│                      │  (Agent PTY)   │  │     │
│              │                      │                │  │     │
│              │                      │ Output Panel   │  │     │
│              │                      │  (图表 + 日志)  │  │     │
└──────────────┴───────────────────────┴────────────────────────┘
```

### 6.2 新增/调整的组件

| 组件 | 说明 |
|------|------|
| `AwarenessOverlay` | Monaco Editor overlay，显示队友光标和选区 |
| `TeamPanel` | Sidebar 里的在线队友列表 |
| `PaperEditor` | 增强的文档编辑器（LaTeX 预览 + KaTeX 渲染） |
| `LiteratureBoard` | 资料收集看板，拖入 AI 搜索结果 |
| `AIChatPanel` | 侧边栏 AI 对话，连接中转站 |
| `HistoryTimeline` | 版本时间线滑块 |
| `AgentTerminal` | 扩展现有 Terminal，支持 PTY 实时流 |
| `DockerEnvPanel` | Docker 环境管理（包安装/重置） |

### 6.3 路由设计

| 路由 | 页面 |
|------|------|
| `/` | 登录/注册 |
| `/projects` | 项目列表 |
| `/projects/[id]` | 项目工作台（三栏布局） |
| `/admin/keys` | API Key 管理（项目 owner） |

---

## 七、部署

**开发环境**：
```bash
# 后端
cd server && cargo watch -x run

# 前端
npm run dev

# Local Agent
cd agent && cargo run -- --server wss://localhost:3000
```

**生产部署**：
```
modeler-ai  (单一二进制)
├── --port 8080
├── --db ./data/modeler.db
├── --data-dir ./data/
└── systemd service or pm2
```

不需要 Docker Compose，不需要 Nginx 反向代理（Axum 自带 TLS 支持，可选）。

---

## 八、开发阶段划分

| 阶段 | 内容 | 预估 |
|------|------|------|
| **Phase 1** | Auth + Project + File 基础 CRUD | 先通后端核心链路 |
| **Phase 2** | Sync (yrs WebSocket) + 前端 Monaco CRDT 集成 | 最核心的实时协同 |
| **Phase 3** | AI 中转站 (one-api 风格) + 前端对话面板 | AI 能力上线 |
| **Phase 4** | Compute (Docker 沙箱) + Terminal WS | 代码执行 |
| **Phase 5** | History (快照+时间线) | 版本管理 |
| **Phase 6** | Local Agent + Claude Code 桥接 + Tabbit 集成 | 本地能力 |
| **Phase 7** | 打磨：权限细化、UI 优化、测试 | 赛前准备 |

---

## 九、风险与注意事项

1. **CRDT 网络分区冲突** — 竞赛网络可能不稳定，yrs 的离线编辑 + 重连合并机制需充分测试
2. **Docker 镜像体积** — 基础镜像需预装常用科学计算库，控制在 2GB 内
3. **Claude Code CLI 版本兼容** — 本地 Agent 需适配不同操作系统的 Claude Code 安装路径
4. **安全性** — 比赛项目间数据隔离、API Key 加密存储、Docker 沙箱逃逸防护
