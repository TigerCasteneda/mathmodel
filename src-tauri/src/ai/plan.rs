#![allow(dead_code)]

use async_trait::async_trait;
use claude_code_rs::mcp::ToolExecutor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

#[derive(Clone)]
pub struct PlanService {
    current_plan: Arc<RwLock<Option<Plan>>>,
    plan_dir: PathBuf,
}

impl PlanService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            current_plan: Arc::new(RwLock::new(None)),
            plan_dir: data_dir.join("plans"),
        }
    }

    pub async fn is_planning(&self) -> bool {
        self.current_plan.read().await.is_some()
    }

    pub async fn current_plan(&self) -> Option<Plan> {
        self.current_plan.read().await.clone()
    }

    pub async fn enter_plan_mode(&self) -> bool {
        if self.is_planning().await {
            return false;
        }
        let plan = Plan {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            phases: vec![],
            status: PlanStatus::Drafting,
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        let mut guard = self.current_plan.write().await;
        *guard = Some(plan);
        true
    }

    pub async fn submit_plan(&self, title: &str, phases: Vec<PlanPhase>) -> Option<Plan> {
        let mut guard = self.current_plan.write().await;
        let plan = guard.as_mut()?;
        plan.title = title.to_string();
        plan.phases = phases.into_iter().map(|p| PlanPhase {
            status: PhaseStatus::Pending,
            ..p
        }).collect();
        plan.status = PlanStatus::Ready;

        // Persist
        if let Some(parent) = self.plan_dir.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let path = self.plan_dir.join(format!("{}.json", plan.id));
        if let Ok(json) = serde_json::to_string_pretty(&*plan) {
            std::fs::write(&path, json).ok();
        }

        Some(plan.clone())
    }

    pub async fn approve_plan(&self) -> Option<Plan> {
        let mut guard = self.current_plan.write().await;
        let plan = guard.as_mut()?;
        if plan.status != PlanStatus::Ready {
            return None;
        }
        plan.status = PlanStatus::Executing;
        Some(plan.clone())
    }

    pub async fn update_phase(
        &self,
        phase_title: &str,
        new_status: PhaseStatus,
    ) -> Option<Plan> {
        let mut guard = self.current_plan.write().await;
        let plan = guard.as_mut()?;
        for phase in &mut plan.phases {
            if phase.title == phase_title {
                phase.status = new_status;
            }
        }
        // Check if all phases completed
        if plan.phases.iter().all(|p| matches!(p.status, PhaseStatus::Completed | PhaseStatus::Skipped))
            && !plan.phases.is_empty()
        {
            plan.status = PlanStatus::Completed;
        }
        Some(plan.clone())
    }

    pub async fn exit_plan_mode(&self) -> Option<Plan> {
        let mut guard = self.current_plan.write().await;
        guard.take()
    }
}

// ── Tool Executors ──

pub struct EnterPlanModeExecutor {
    pub plan_service: Arc<PlanService>,
}

#[async_trait]
impl ToolExecutor for EnterPlanModeExecutor {
    async fn execute(&self, _input: Value) -> anyhow::Result<Value> {
        if self.plan_service.enter_plan_mode().await {
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
}

#[async_trait]
impl ToolExecutor for ExitPlanModeExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let title = input["title"].as_str().unwrap_or("Untitled Plan");
        let phases: Vec<PlanPhase> = input["phases"]
            .as_array()
            .map(|arr| {
                arr.iter().map(|p| PlanPhase {
                    title: p["title"].as_str().unwrap_or("").to_string(),
                    steps: p["steps"].as_array()
                        .map(|s| s.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                    status: PhaseStatus::Pending,
                }).collect()
            })
            .unwrap_or_default();

        match self.plan_service.submit_plan(title, phases).await {
            Some(plan) => Ok(serde_json::to_value(&plan)?),
            None => Ok(json!({ "error": "No active plan mode" })),
        }
    }
}

pub struct PlanUpdateExecutor {
    pub plan_service: Arc<PlanService>,
}

#[async_trait]
impl ToolExecutor for PlanUpdateExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let phase_title = input["phase"].as_str()
            .ok_or_else(|| anyhow::anyhow!("phase required"))?;
        let status_str = input["status"].as_str().unwrap_or("completed");
        let status = match status_str {
            "in_progress" => PhaseStatus::InProgress,
            "completed" => PhaseStatus::Completed,
            "skipped" => PhaseStatus::Skipped,
            _ => PhaseStatus::Pending,
        };
        match self.plan_service.update_phase(phase_title, status).await {
            Some(plan) => Ok(serde_json::to_value(&plan)?),
            None => Ok(json!({ "error": "No active plan" })),
        }
    }
}
