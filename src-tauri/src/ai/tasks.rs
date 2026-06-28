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

/// Per-user per-conversation task list store. The previous layout
/// stored tasks under `task-lists/<conversation_id>.json` with no
/// user_id, so two accounts using the same conversation_id (e.g. the
/// "default" fallback) shared a single file and overwrote each
/// other's todo list. The `Task.subject` / `description` / `tags`
/// fields are user-authored, so a leak here is real content
/// exposure.
pub struct TaskStore {
    user_id: String,
    conversation_id: String,
    data_dir: PathBuf,
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

impl TaskStore {
    pub fn new(user_id: String, conversation_id: String, data_dir: PathBuf) -> Self {
        Self {
            user_id,
            conversation_id,
            data_dir,
        }
    }

    fn file_path(&self) -> PathBuf {
        self.data_dir
            .join("task-lists")
            .join(sanitize_user_id(&self.user_id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("modeler-tasks-{label}-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tasks_are_isolated_per_user() {
        // Same conversation_id, two different users. Alice's task list
        // and Bob's task list must live in separate files.
        let root = unique_tmp_dir("per-user-tasks");
        let alice = TaskStore::new("user-alice".into(), "conv-1".into(), root.clone());
        let bob = TaskStore::new("user-bob".into(), "conv-1".into(), root.clone());

        alice
            .create("Alice todo", "Alice's secret plan", None, None, None);
        bob.create("Bob todo", "Bob's secret plan", None, None, None);

        let alice_list = alice.list(None, None);
        let bob_list = bob.list(None, None);
        assert_eq!(alice_list.len(), 1);
        assert_eq!(bob_list.len(), 1);
        assert_eq!(alice_list[0].subject, "Alice todo");
        assert_eq!(bob_list[0].subject, "Bob todo");

        // Disk layout: per-user dir under task-lists/.
        assert!(root
            .join("task-lists")
            .join("user-alice")
            .join("conv-1.json")
            .exists());
        assert!(root
            .join("task-lists")
            .join("user-bob")
            .join("conv-1.json")
            .exists());
        assert!(!root
            .join("task-lists")
            .join("user-bob")
            .join("user-alice")
            .exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
