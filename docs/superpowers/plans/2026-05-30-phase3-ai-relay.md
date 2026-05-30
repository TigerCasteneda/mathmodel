# Phase 3: AI Relay Implementation Plan

> Reference implementation: `D:/5_user/mathmodel/one-api` (Go)
> Scope: learn one-api's routing and channel ideas, do not copy the whole system.

## Goal

Build the AI capability layer for Modeler AI: a Rust/Axum relay that exposes OpenAI-compatible APIs, routes requests to multiple upstream providers, manages provider keys, enforces project quotas, and records usage logs.

This is not just a chat box. It is the control point for all future AI features: literature search, modeling assistance, paper writing, code generation, compute-result explanation, and later local-agent workflows.

## Design Position

Phase 3a should implement a small, high-performance subset of one-api:

- Channel management: upstream provider config, model list, weight, status.
- Model routing: choose a healthy channel for a requested model.
- Adaptors: normalize provider-specific APIs into OpenAI-compatible responses.
- Project quota: charge usage to a project, not a global bucket.
- Audit log: record user, project, model, channel, status, tokens, latency, and error.

Do not implement one-api's users, groups, redemption, dashboard, billing UI, external token system, or full provider matrix in this phase. Auth already exists in Phase 1.

## Non-Goals For Phase 3a

- No streaming responses. If `stream=true`, return `400 BadRequest("streaming is not supported yet")`.
- No provider health monitor or automatic cooldown yet.
- No real API key encryption yet. Store plaintext in a field named `api_key` for dev, and document that encryption is Phase 3b.
- No public API tokens. Use the existing JWT access token.
- No full admin UI. REST admin endpoints are enough for now.

## one-api Mapping

| one-api concept | Modeler AI implementation | Notes |
|---|---|---|
| Channel | `ai/channel.rs` + `channels` table | Provider config and model ability list |
| Token | existing JWT | No separate token table in Phase 3a |
| Ability | channel `models` field | Comma-separated exact model names; `*` means all |
| Adaptor | `ai/adaptor/*` | Provider-specific request/response conversion |
| Log | `ai_usage_logs` | Project-scoped audit trail |
| Quota | `project_quotas` | Project-level token budget |

## File Structure

```text
server/src/ai/
  mod.rs
  model.rs
  handlers.rs
  channel.rs
  quota.rs
  adaptor/
    mod.rs
    openai.rs
    anthropic.rs
    tavily.rs
```

## Task 1: Dependencies And Migration

Files:

- Update: `server/Cargo.toml`
- Create: `server/migrations/002_ai.sql`
- Update: `server/src/db.rs`

Add dependencies:

```toml
async-trait = "0.1"
rand = "0.8"
```

Create `server/migrations/002_ai.sql`:

```sql
CREATE TABLE IF NOT EXISTS channels (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    channel_type INTEGER NOT NULL,
    base_url TEXT NOT NULL DEFAULT '',
    api_key TEXT NOT NULL DEFAULT '',
    models TEXT NOT NULL DEFAULT '',
    model_mapping TEXT NOT NULL DEFAULT '{}',
    weight INTEGER NOT NULL DEFAULT 1,
    status INTEGER NOT NULL DEFAULT 1,
    config TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS project_quotas (
    project_id TEXT PRIMARY KEY REFERENCES projects(id),
    total_tokens_used INTEGER NOT NULL DEFAULT 0,
    token_limit INTEGER NOT NULL DEFAULT 100000000,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_usage_logs (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    project_id TEXT NOT NULL REFERENCES projects(id),
    channel_id TEXT REFERENCES channels(id),
    model TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'success',
    error_message TEXT,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_channels_status ON channels(status);
CREATE INDEX IF NOT EXISTS idx_ai_usage_project_created ON ai_usage_logs(project_id, created_at);
CREATE INDEX IF NOT EXISTS idx_ai_usage_user_created ON ai_usage_logs(user_id, created_at);
```

Update `db.rs` so startup runs both migrations in order:

```rust
async fn run_migrations(pool: &SqlitePool) {
    run_specific_migration(pool, include_str!("../migrations/001_initial.sql")).await;
    run_specific_migration(pool, include_str!("../migrations/002_ai.sql")).await;
}

pub async fn run_specific_migration(pool: &SqlitePool, sql: &str) {
    for statement in sql.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("Failed to run migration");
    }
}
```

Verification:

```powershell
cd D:\5_user\mathmodel\server
cargo check
```

## Task 2: AI Models

Files:

- Create: `server/src/ai/mod.rs`
- Create: `server/src/ai/model.rs`

`mod.rs`:

```rust
pub mod adaptor;
pub mod channel;
pub mod handlers;
pub mod model;
pub mod quota;
```

Important model requirements:

- `Channel` must derive `Clone`, because routing returns a selected channel.
- `ChatCompletionResponse`, `Choice`, and `Usage` must derive both `Serialize` and `Deserialize`, because handlers parse adaptor JSON back into typed responses.
- Requests must carry `project_id`; quota and logs are project-scoped.

Core structs:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub channel_type: i32,
    pub base_url: String,
    pub api_key: String,
    pub models: String,
    pub model_mapping: String,
    pub weight: i32,
    pub status: i32,
    pub config: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Channel {
    pub fn parsed_models(&self) -> Vec<String> {
        self.models
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn supports_model(&self, model: &str) -> bool {
        self.models.trim() == "*" || self.parsed_models().iter().any(|m| m == model)
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub channel_type: i32,
    pub base_url: String,
    pub api_key: String,
    pub models: String,
    pub model_mapping: Option<String>,
    pub weight: Option<i32>,
    pub config: Option<String>,
}

pub mod channel_type {
    pub const OPENAI_COMPATIBLE: i32 = 1;
    pub const ANTHROPIC: i32 = 18;
    pub const TAVILY: i32 = 99;
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub project_id: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i32>,
    pub stream: Option<bool>,
    pub top_p: Option<f64>,
    pub n: Option<i32>,
    pub stop: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Choice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub project_id: String,
    pub query: String,
    pub max_results: Option<i32>,
}
```

## Task 3: Channel Routing

Files:

- Create: `server/src/ai/channel.rs`

Routing rules:

- Load enabled channels with `status = 1`.
- Match exact models via `Channel::supports_model`, not SQL `LIKE`.
- Pick randomly among top-weight channels.
- Validate channel input before insert: non-empty name, base_url, api_key, models, positive weight.

Avoid this bug:

```rust
models LIKE '%gpt-4%'
```

It will incorrectly match unrelated model names. Filter in Rust with exact comma-separated model matching until a better `channel_models` table exists.

## Task 4: Quota

Files:

- Create: `server/src/ai/quota.rs`

Rules:

- Quota is project-scoped.
- Check membership before checking quota.
- If no quota row exists, insert one lazily with default limit.
- Do not use `"global"` as a fake project ID.
- Deduct actual usage after response parsing.
- If usage is missing, deduct a conservative estimate based on request length and `max_tokens`.

Required functions:

```rust
pub async fn ensure_project_member(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    user_id: &str,
) -> Result<(), AppError>;

pub async fn check_quota(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    estimated_tokens: i32,
) -> Result<(), AppError>;

pub async fn deduct_quota(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    tokens: i32,
) -> Result<(), AppError>;
```

## Task 5: Adaptors

Files:

- Create: `server/src/ai/adaptor/mod.rs`
- Create: `server/src/ai/adaptor/openai.rs`
- Create: `server/src/ai/adaptor/anthropic.rs`
- Create: `server/src/ai/adaptor/tavily.rs`

Trait:

```rust
use async_trait::async_trait;
use crate::ai::model::*;
use crate::error::AppError;

#[async_trait]
pub trait Adaptor: Send + Sync {
    fn build_url(&self, base_url: &str) -> String;
    fn build_headers(&self, api_key: &str) -> Vec<(String, String)>;
    fn convert_request(
        &self,
        req: &ChatCompletionRequest,
        upstream_model: &str,
    ) -> Result<serde_json::Value, AppError>;
    async fn parse_response(&self, body: &str) -> Result<(serde_json::Value, Usage), AppError>;
    fn provider_name(&self) -> &str;
}
```

Provider notes:

- OpenAI-compatible providers use `{base_url}/chat/completions`; configure `base_url` as `https://api.deepseek.com/v1` or equivalent.
- Anthropic uses `{base_url}/messages`; convert system messages into `system`.
- Tavily should use its native search request. Put `api_key` into the request body or use Tavily's accepted auth header; do not leave `"api_key": ""`.
- `model_mapping` should map public model name to upstream model name. If absent, use request model unchanged.

## Task 6: Handlers

Files:

- Create: `server/src/ai/handlers.rs`
- Update: `server/src/main.rs`

Routes:

```rust
Router::new()
    .route("/v1/chat/completions", post(chat_completions))
    .route("/v1/models", get(list_models))
    .route("/search", post(search))
    .route("/admin/channels", get(admin_list_channels).post(admin_create_channel))
```

Mount:

```rust
.nest("/ai", ai::handlers::routes())
```

Handler flow:

1. Extract `AuthUser`.
2. Reject `stream=true`.
3. Validate `project_id` membership.
4. Estimate tokens and check project quota.
5. Select channel.
6. Apply `model_mapping`.
7. Build upstream request through adaptor.
8. Send with a shared `reqwest::Client` or a per-request client with timeout.
9. Parse upstream response.
10. Log success or failure.
11. Deduct quota on success.
12. Return OpenAI-compatible response.

Error policy:

- Bad client input: `400`.
- No channel/model: `404`.
- Unauthorized/missing token: `401`.
- Not project member: `403`.
- Upstream failure: return `502` in Phase 3b; for Phase 3a, use `AppError::Internal` but include a short sanitized upstream status.

Admin endpoints:

- In Phase 3a, only project owners may create/list channels.
- Use an `is_project_owner(pool, project_id, user_id)` helper and require `project_id` in admin create/list requests, or add a simple global admin flag later.
- Do not allow any logged-in user to add provider keys.

## Task 7: Frontend Integration

Phase 3a frontend should be thin:

- Add `lib/ai-api.ts` with typed wrappers for:
  - `listModels()`
  - `chatCompletions(projectId, model, messages)`
  - `search(projectId, query)`
- Keep the existing mock `MainWorkspace` UI, but wire one minimal "Ask" action to `/ai/v1/chat/completions`.
- Full `AIChatPanel`, citations board, model picker, and admin channel UI can be Phase 3b.

Do not wire AI calls directly inside low-level UI components. Keep API calls in `lib/ai-api.ts` so later UI changes do not touch relay details.

## Task 8: Verification

Backend checks:

```powershell
cd D:\5_user\mathmodel\server
cargo fmt --check
cargo check
cargo clippy --all-targets
cargo test -j 1
```

Frontend check:

```powershell
cd D:\5_user\mathmodel
cmd /c npx tsc --noEmit
```

Manual smoke test:

1. Register/login user A.
2. Create project.
3. Create channel for `deepseek-chat` or a local mock upstream.
4. Call `/ai/v1/models`; verify model appears.
5. Call `/ai/v1/chat/completions` with `project_id`; verify response and usage log.
6. Lower project quota and verify the next request is rejected.
7. Login user B who is not a project member; verify chat request returns 403.

Example:

```bash
curl -X POST http://localhost:3001/ai/admin/channels \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "project_id": "PROJECT_ID",
    "name": "DeepSeek",
    "channel_type": 1,
    "base_url": "https://api.deepseek.com/v1",
    "api_key": "sk-xxx",
    "models": "deepseek-chat",
    "weight": 1
  }'

curl -X POST http://localhost:3001/ai/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "project_id": "PROJECT_ID",
    "model": "deepseek-chat",
    "messages": [{"role": "user", "content": "Say hello"}]
  }'
```

## Implementation Notes

- Prefer a modular monolith. Do not split this into services.
- Keep SQL concentrated in `ai/channel.rs`, `ai/quota.rs`, and `ai/handlers.rs`.
- Keep the adaptor trait small. Provider quirks belong in adaptors, not handlers.
- Make all AI requests project-contextual from the start.
- Use exact model matching now; introduce a normalized `channel_models` table only if model management grows.
- Phase 3b can add encryption, streaming, channel health, fallback/cooldown, admin UI, and richer cost accounting.
