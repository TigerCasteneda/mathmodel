#![allow(dead_code)]

use async_trait::async_trait;
use claude_code_rs::mcp::ToolExecutor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Drafting,
    Ready,
    Approved,
    Executing,
    Completed,
    Rejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum PhaseStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPhase {
    pub title: String,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub status: PhaseStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub phases: Vec<PlanPhase>,
    pub status: PlanStatus,
    pub created_at: i64,
}

/// Per-user plan service. Previously a single
/// `Arc<RwLock<Option<Plan>>>` was shared across the entire Tauri
/// process, so User A entering plan mode would block User B's
/// `enter_plan_mode` (which returns `false` while a plan is active)
/// and on logout/re-login, User B would inherit User A's in-flight
/// plan. Now `current_plan` is a per-user `HashMap` and on-disk
/// plans live under `data_dir/plans/<user_id>/<plan_id>.json`.
#[derive(Clone)]
pub struct PlanService {
    /// user_id -> active plan, if any. A separate per-user `Option<Plan>`
    /// means two accounts can plan in parallel without stepping on each
    /// other.
    current_plan: Arc<RwLock<HashMap<String, Option<Plan>>>>,
    /// Root directory that holds per-user subdirectories.
    plan_root: PathBuf,
}

fn sanitize_user_id(user_id: &str) -> String {
    let cleaned: String = user_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

impl PlanService {
    pub fn new(data_dir: PathBuf) -> Self {
        // data_dir/plans/ — each user's plans/<user_id>/<plan_id>.json
        let plan_root = data_dir.join("plans");
        let _ = std::fs::create_dir_all(&plan_root);

        // One-shot migration: the previous layout stored plans at
        // `data_dir/plans/<plan_id>.json` with no user scoping. We
        // can't safely attribute any pre-existing plan to a user, so
        // drop them. Consistent with the chat-sessions / hooks
        // migration policy: user chose 'delete old data' for chat.
        if let Ok(entries) = std::fs::read_dir(&plan_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_dir = path
                    .metadata()
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
                if !is_dir {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }

        Self {
            current_plan: Arc::new(RwLock::new(HashMap::new())),
            plan_root,
        }
    }

    fn user_dir(&self, user_id: &str) -> PathBuf {
        self.plan_root.join(sanitize_user_id(user_id))
    }

    fn plan_path(&self, user_id: &str, plan_id: &str) -> PathBuf {
        self.user_dir(user_id).join(format!("{plan_id}.json"))
    }

    pub async fn is_planning(&self, user_id: &str) -> bool {
        let map = self.current_plan.read().await;
        map.get(user_id).and_then(|p| p.as_ref()).is_some()
    }

    pub async fn current_plan(&self, user_id: &str) -> Option<Plan> {
        let map = self.current_plan.read().await;
        map.get(user_id).and_then(|p| p.clone())
    }

    pub async fn enter_plan_mode(&self, user_id: &str) -> bool {
        if self.is_planning(user_id).await {
            return false;
        }
        let plan = Plan {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            phases: vec![],
            status: PlanStatus::Drafting,
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        let mut map = self.current_plan.write().await;
        map.insert(user_id.to_string(), Some(plan));
        true
    }

    pub async fn submit_plan(
        &self,
        user_id: &str,
        title: &str,
        phases: Vec<PlanPhase>,
    ) -> Option<Plan> {
        let plan = {
            let mut map = self.current_plan.write().await;
            let entry = map.get_mut(user_id)?;
            let plan = entry.as_mut()?;
            plan.title = title.to_string();
            plan.phases = phases
                .into_iter()
                .map(|p| PlanPhase {
                    status: PhaseStatus::Pending,
                    ..p
                })
                .collect();
            plan.status = PlanStatus::Ready;
            plan.clone()
        };

        // Persist under plans/<user_id>/<plan_id>.json so the plan
        // survives across Tauri restarts and user_id switch (User B
        // re-logging in does not pick up User A's plans).
        let user_dir = self.user_dir(user_id);
        std::fs::create_dir_all(&user_dir).ok();
        let path = self.plan_path(user_id, &plan.id);
        if let Ok(json) = serde_json::to_string_pretty(&plan) {
            let _ = std::fs::write(&path, json);
        }

        Some(plan)
    }

    pub async fn approve_plan(&self, user_id: &str) -> Option<Plan> {
        let mut map = self.current_plan.write().await;
        let plan = map.get_mut(user_id)?.as_mut()?;
        if plan.status != PlanStatus::Ready {
            return None;
        }
        plan.status = PlanStatus::Executing;
        Some(plan.clone())
    }

    pub async fn update_phase(
        &self,
        user_id: &str,
        phase_title: &str,
        new_status: PhaseStatus,
    ) -> Option<Plan> {
        let mut map = self.current_plan.write().await;
        let plan = map.get_mut(user_id)?.as_mut()?;
        for phase in &mut plan.phases {
            if phase.title == phase_title {
                phase.status = new_status;
            }
        }
        // Check if all phases completed
        if plan
            .phases
            .iter()
            .all(|p| matches!(p.status, PhaseStatus::Completed | PhaseStatus::Skipped))
            && !plan.phases.is_empty()
        {
            plan.status = PlanStatus::Completed;
        }
        Some(plan.clone())
    }

    pub async fn exit_plan_mode(&self, user_id: &str) -> Option<Plan> {
        let mut map = self.current_plan.write().await;
        map.get_mut(user_id)?.take()
    }
}

// ── Tool Executors ──
//
// Each executor captures the user_id of the runtime that built it so
// plan operations are scoped to whichever account is in chat. (The
// runtime is constructed per `ai_chat` call, and `ai_chat` already
// receives `user_id` from the frontend.)

pub struct EnterPlanModeExecutor {
    pub plan_service: Arc<PlanService>,
    pub user_id: String,
}

#[async_trait]
impl ToolExecutor for EnterPlanModeExecutor {
    async fn execute(&self, _input: Value) -> anyhow::Result<Value> {
        if self.plan_service.enter_plan_mode(&self.user_id).await {
            Ok(json!({
                "success": true,
                "message": "Plan mode entered. Use read-only tools to explore and design. Call exit_plan_mode with a structured plan when ready."
            }))
        } else {
            Ok(json!({ "success": false, "message": "Already in plan mode" }))
        }
    }
}

pub struct ExitPlanModeExecutor {
    pub plan_service: Arc<PlanService>,
    pub user_id: String,
}

#[async_trait]
impl ToolExecutor for ExitPlanModeExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let title = input["title"].as_str().unwrap_or("Untitled Plan");
        let phases: Vec<PlanPhase> = input["phases"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|p| PlanPhase {
                        title: p["title"].as_str().unwrap_or("").to_string(),
                        steps: p["steps"]
                            .as_array()
                            .map(|s| {
                                s.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        status: PhaseStatus::Pending,
                    })
                    .collect()
            })
            .unwrap_or_default();

        match self.plan_service.submit_plan(&self.user_id, title, phases).await {
            Some(plan) => Ok(serde_json::to_value(&plan)?),
            None => Ok(json!({ "error": "No active plan mode" })),
        }
    }
}

pub struct PlanUpdateExecutor {
    pub plan_service: Arc<PlanService>,
    pub user_id: String,
}

#[async_trait]
impl ToolExecutor for PlanUpdateExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let phase_title = input["phase"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("phase required"))?;
        let status_str = input["status"].as_str().unwrap_or("completed");
        let status = match status_str {
            "in_progress" => PhaseStatus::InProgress,
            "completed" => PhaseStatus::Completed,
            "skipped" => PhaseStatus::Skipped,
            _ => PhaseStatus::Pending,
        };
        match self
            .plan_service
            .update_phase(&self.user_id, phase_title, status)
            .await
        {
            Some(plan) => Ok(serde_json::to_value(&plan)?),
            None => Ok(json!({ "error": "No active plan" })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{sanitize_user_id, PlanService};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("modeler-plan-{label}-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sanitize_user_id_basic() {
        assert_eq!(sanitize_user_id("user-abc_123"), "user-abc_123");
        assert_eq!(sanitize_user_id("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_user_id(""), "unknown");
    }

    #[test]
    fn new_drops_legacy_plans() {
        let root = unique_tmp_dir("legacy-plans");
        let plans = root.join("plans");
        std::fs::create_dir_all(&plans).unwrap();
        // A pre-existing unscoped plan file.
        std::fs::write(
            plans.join("legacy-plan.json"),
            r#"{"id":"legacy-plan","title":"x","phases":[],"status":"drafting","created_at":1}"#,
        )
        .unwrap();

        let _service = PlanService::new(root.clone());

        assert!(
            !plans.join("legacy-plan.json").exists(),
            "legacy plan file should be removed on startup"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn current_plan_is_isolated_per_user() {
        let root = unique_tmp_dir("per-user-plan");
        let service = PlanService::new(root.clone());

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Alice enters plan mode and is mid-plan.
            assert!(service.enter_plan_mode("user-alice").await);
            assert!(service.is_planning("user-alice").await);

            // Bob's `enter_plan_mode` must succeed even though Alice is
            // still mid-plan — the previous single-Option design would
            // have returned false here.
            assert!(
                service.enter_plan_mode("user-bob").await,
                "Bob should be able to enter plan mode while Alice is mid-plan"
            );

            // Alice's current plan and Bob's current plan are distinct slots.
            let alice_plan = service.current_plan("user-alice").await.unwrap();
            let bob_plan = service.current_plan("user-bob").await.unwrap();
            assert_ne!(alice_plan.id, bob_plan.id);

            // Exiting Bob's plan must NOT clear Alice's.
            service.exit_plan_mode("user-bob").await;
            assert!(service.is_planning("user-alice").await);
            assert!(!service.is_planning("user-bob").await);
        });

        let _ = std::fs::remove_dir_all(&root);
    }
}
