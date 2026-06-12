use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl TaskStatus {
    pub fn from_str(s: &str) -> Self {
        match s {
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "deleted" => Self::Deleted,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl TaskPriority {
    pub fn from_str(s: &str) -> Self {
        match s {
            "medium" => Self::Medium,
            "high" => Self::High,
            "critical" => Self::Critical,
            _ => Self::Low,
        }
    }
}

pub struct TaskStore {
    conversation_id: String,
    data_dir: PathBuf,
}

impl TaskStore {
    pub fn new(conversation_id: String, data_dir: PathBuf) -> Self {
        Self {
            conversation_id,
            data_dir,
        }
    }

    fn file_path(&self) -> PathBuf {
        self.data_dir
            .join("task-lists")
            .join(format!("{}.json", self.conversation_id))
    }

    fn generate_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    fn load_tasks(&self) -> Vec<Task> {
        let path = self.file_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    fn save_tasks(&self, tasks: &[Task]) {
        let path = self.file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(json) = serde_json::to_string_pretty(tasks) {
            std::fs::write(&path, json).ok();
        }
    }

    pub fn create(
        &self,
        subject: &str,
        description: &str,
        priority: Option<&str>,
        blocks: Option<Vec<String>>,
        tags: Option<Vec<String>>,
    ) -> Task {
        let now = Self::now_ms();
        let mut tasks = self.load_tasks();
        let task = Task {
            id: Self::generate_id(),
            subject: subject.to_string(),
            description: description.to_string(),
            status: TaskStatus::Pending,
            priority: priority
                .map(TaskPriority::from_str)
                .unwrap_or(TaskPriority::Medium),
            blocks: blocks.unwrap_or_default(),
            blocked_by: Vec::new(),
            tags: tags.unwrap_or_default(),
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        };
        tasks.push(task.clone());
        self.save_tasks(&tasks);
        task
    }

    pub fn update(
        &self,
        task_id: &str,
        status: Option<&str>,
        subject: Option<&str>,
        description: Option<&str>,
        priority: Option<&str>,
    ) -> Option<Task> {
        let mut tasks = self.load_tasks();
        let pos = tasks.iter().position(|t| t.id == task_id)?;
        let now = Self::now_ms();
        if let Some(s) = status {
            tasks[pos].status = TaskStatus::from_str(s);
        }
        if let Some(s) = subject {
            tasks[pos].subject = s.to_string();
        }
        if let Some(d) = description {
            tasks[pos].description = d.to_string();
        }
        if let Some(p) = priority {
            tasks[pos].priority = TaskPriority::from_str(p);
        }
        tasks[pos].updated_at = now;
        let updated = tasks[pos].clone();
        self.save_tasks(&tasks);
        Some(updated)
    }

    pub fn list(&self, status: Option<&str>, priority: Option<&str>) -> Vec<Task> {
        self.load_tasks()
            .into_iter()
            .filter(|t| {
                t.status != TaskStatus::Deleted
                    && status
                        .map(|s| TaskStatus::from_str(s) == t.status)
                        .unwrap_or(true)
                    && priority
                        .map(|p| TaskPriority::from_str(p) == t.priority)
                        .unwrap_or(true)
            })
            .collect()
    }

    pub fn get(&self, task_id: &str) -> Option<Task> {
        self.load_tasks()
            .into_iter()
            .find(|t| t.id == task_id && t.status != TaskStatus::Deleted)
    }
}
