//! Operation History — tracks tool calls with success/failure and timing.
//!
//! Adapted from Claude Code's `memory::history::HistoryManager`, scoped to the
//! Modeler AI app: one JSON file per (user, session) under
//! `<app_data>/operation-history/<user_id>/<session_id>.json`. Max 500 entries
//! per session (oldest evicted first). Per-user scoping prevents User B
//! from seeing User A's tool-call audit trail via the search() aggregate.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_ENTRIES_PER_SESSION: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    ToolCall,
    FileRead,
    FileWrite,
    WebSearch,
    FetchUrl,
    ExecuteCommand,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationEntry {
    pub id: String,
    /// Authenticated user who triggered this operation. The Rust side
    /// stamps it on `record` and the search() method filters by it; a
    /// client cannot forge it from the frontend because the Tauri
    /// command takes user_id separately and overrides the field.
    pub user_id: String,
    pub session_id: String,
    pub op_type: OperationType,
    pub tool_name: String,
    /// First ~200 chars of the tool arguments for display.
    pub input_preview: String,
    pub success: bool,
    pub duration_ms: u64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationStats {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
    pub avg_duration_ms: u64,
    pub top_tools: Vec<ToolCount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCount {
    pub tool_name: String,
    pub count: usize,
}

/// Persisted operation history store. Layout is
/// `<app_data>/operation-history/<user_id>/<session_id>.json`; the
/// `search` aggregate is restricted to the calling user's subtree.
pub struct OperationHistoryStore {
    history_root: PathBuf,
    /// (user_id, session_id) -> entries
    cache: Mutex<HashMap<(String, String), Vec<OperationEntry>>>,
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

impl OperationHistoryStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let history_root = app_data_dir.join("operation-history");
        let _ = std::fs::create_dir_all(&history_root);

        // One-shot migration: the previous layout stored
        // `<history_root>/<session_id>.json` with no user_id. Two
        // accounts using the same `conversation_id` (e.g. the
        // "default" fallback) would have over-written each other.
        // Drop the loose files; we can't safely attribute them.
        // Consistent with chat / hooks / plans / plugins / permissions
        // migration policy: 'delete old data'.
        if let Ok(entries) = std::fs::read_dir(&history_root) {
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
            history_root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn user_dir(&self, user_id: &str) -> PathBuf {
        self.history_root.join(sanitize_user_id(user_id))
    }

    fn session_path(&self, user_id: &str, session_id: &str) -> PathBuf {
        self.user_dir(user_id)
            .join(format!("{session_id}.json"))
    }

    fn key(user_id: &str, session_id: &str) -> (String, String) {
        (user_id.to_string(), session_id.to_string())
    }

    fn ensure_loaded(&self, user_id: &str, session_id: &str) -> Result<(), String> {
        let key = Self::key(user_id, session_id);
        let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
        if cache.contains_key(&key) {
            return Ok(());
        }
        let path = self.session_path(user_id, session_id);
        let entries: Vec<OperationEntry> = if path.exists() {
            let content =
                std::fs::read_to_string(&path).map_err(|e| format!("read history: {e}"))?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };
        cache.insert(key, entries);
        Ok(())
    }

    fn persist(&self, user_id: &str, session_id: &str) -> Result<(), String> {
        let key = Self::key(user_id, session_id);
        let cache = self.cache.lock().map_err(|e| e.to_string())?;
        if let Some(entries) = cache.get(&key) {
            let path = self.session_path(user_id, session_id);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let content = serde_json::to_string_pretty(entries)
                .map_err(|e| format!("serialize history: {e}"))?;
            std::fs::write(&path, &content)
                .map_err(|e| format!("write history: {e}"))?;
        }
        Ok(())
    }

    /// Record an operation and persist. The caller's `entry.user_id`
    /// is overwritten with the `user_id` argument — clients cannot
    /// forge attribution by passing an entry with someone else's id.
    pub fn record(&self, user_id: &str, mut entry: OperationEntry) -> Result<(), String> {
        let session_id = entry.session_id.clone();
        entry.user_id = user_id.to_string();
        self.ensure_loaded(user_id, &session_id)?;
        {
            let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
            let entries = cache
                .get_mut(&Self::key(user_id, &session_id))
                .ok_or_else(|| "history cache miss".to_string())?;
            entries.push(entry);
            // FIFO eviction
            while entries.len() > MAX_ENTRIES_PER_SESSION {
                entries.remove(0);
            }
        }
        self.persist(user_id, &session_id)
    }

    /// List all operations for a session (newest first), scoped to a user.
    pub fn list(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Vec<OperationEntry>, String> {
        self.ensure_loaded(user_id, session_id)?;
        let cache = self.cache.lock().map_err(|e| e.to_string())?;
        let mut entries = cache
            .get(&Self::key(user_id, session_id))
            .cloned()
            .unwrap_or_default();
        entries.reverse(); // newest first
        Ok(entries)
    }

    /// Search only the calling user's operations. Previously this
    /// walked every file in `<history_root>/` and returned results
    /// across all users, so User B could discover User A's tool
    /// invocations (including their arguments' previews).
    #[allow(dead_code)]
    pub fn search(
        &self,
        user_id: &str,
        query: &str,
    ) -> Result<Vec<OperationEntry>, String> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        let user_dir = self.user_dir(user_id);
        if let Ok(dir_entries) = std::fs::read_dir(&user_dir) {
            for entry in dir_entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(entries) =
                        serde_json::from_str::<Vec<OperationEntry>>(&content)
                    {
                        for op in entries {
                            if op.tool_name.to_lowercase().contains(&query)
                                || op.input_preview.to_lowercase().contains(&query)
                            {
                                results.push(op);
                            }
                        }
                    }
                }
            }
        }
        results.sort_by_key(|op| -op.timestamp);
        Ok(results)
    }

    /// Compute stats for a session.
    pub fn stats(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<OperationStats, String> {
        self.ensure_loaded(user_id, session_id)?;
        let cache = self.cache.lock().map_err(|e| e.to_string())?;
        let entries = cache
            .get(&Self::key(user_id, session_id))
            .cloned()
            .unwrap_or_default();

        let total = entries.len();
        let successful = entries.iter().filter(|e| e.success).count();
        let failed = total - successful;
        let avg_duration_ms = if total > 0 {
            entries.iter().map(|e| e.duration_ms).sum::<u64>() / total as u64
        } else {
            0
        };

        let mut tool_counts: HashMap<String, usize> = HashMap::new();
        for entry in &entries {
            *tool_counts
                .entry(entry.tool_name.clone())
                .or_default() += 1;
        }
        let mut top_tools: Vec<ToolCount> = tool_counts
            .into_iter()
            .map(|(tool_name, count)| ToolCount { tool_name, count })
            .collect();
        top_tools.sort_by_key(|t| -(t.count as i64));
        top_tools.truncate(10);

        Ok(OperationStats {
            total,
            successful,
            failed,
            avg_duration_ms,
            top_tools,
        })
    }

    /// Delete history for a session (e.g. when session is deleted).
    #[allow(dead_code)]
    pub fn delete_session_history(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<(), String> {
        let path = self.session_path(user_id, session_id);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("delete history: {e}"))?;
        }
        let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
        cache.remove(&Self::key(user_id, session_id));
        Ok(())
    }
}

/// Tauri commands ─────────────────────────────────────────────

#[tauri::command]
pub fn list_operations(
    user_id: String,
    session_id: String,
    store: tauri::State<'_, OperationHistoryStore>,
) -> Result<Vec<OperationEntry>, String> {
    store.list(&user_id, &session_id)
}

#[tauri::command]
pub fn get_operation_stats(
    user_id: String,
    session_id: String,
    store: tauri::State<'_, OperationHistoryStore>,
) -> Result<OperationStats, String> {
    store.stats(&user_id, &session_id)
}

/// Classify a tool name into an OperationType for display grouping.
pub fn classify_operation(tool_name: &str) -> OperationType {
    match tool_name {
        "file_read" | "read_file" => OperationType::FileRead,
        "file_write" | "write_file" | "file_edit" => OperationType::FileWrite,
        "web_search" | "web_search_quick" | "search_academic" | "search_web" => {
            OperationType::WebSearch
        }
        "fetch_url" | "fetch_urls" | "extract_structured" => OperationType::FetchUrl,
        "execute_command" => OperationType::ExecuteCommand,
        _ if tool_name.starts_with("tool_") || tool_name.contains("search") => OperationType::ToolCall,
        _ => OperationType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_store() -> OperationHistoryStore {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("modeler-history-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        OperationHistoryStore::new(dir)
    }

    fn make_entry(user_id: &str, session_id: &str, tool_name: &str, success: bool) -> OperationEntry {
        OperationEntry {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            op_type: classify_operation(tool_name),
            tool_name: tool_name.to_string(),
            input_preview: format!(r#"{{"query":"test {tool_name}"}}"#),
            success,
            duration_ms: 42,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    #[test]
    fn record_and_list_roundtrips() {
        let store = test_store();
        store.record("user-alice", make_entry("user-alice", "s1", "file_read", true)).unwrap();
        store.record("user-alice", make_entry("user-alice", "s1", "web_search", true)).unwrap();
        store.record("user-alice", make_entry("user-alice", "s1", "execute_command", false)).unwrap();
        store.record("user-alice", make_entry("user-alice", "s2", "fetch_url", true)).unwrap();

        let s1 = store.list("user-alice", "s1").unwrap();
        assert_eq!(s1.len(), 3, "s1 should have 3 entries (newest first)");

        let s2 = store.list("user-alice", "s2").unwrap();
        assert_eq!(s2.len(), 1);
    }

    #[test]
    fn stats_computes_correctly() {
        let store = test_store();
        for _ in 0..4 {
            store.record(
                "user-alice",
                make_entry("user-alice", "stats-session", "file_read", true),
            )
            .unwrap();
        }
        store.record(
            "user-alice",
            make_entry("user-alice", "stats-session", "execute_command", false),
        )
        .unwrap();

        let s = store.stats("user-alice", "stats-session").unwrap();
        assert_eq!(s.total, 5);
        assert_eq!(s.successful, 4);
        assert_eq!(s.failed, 1);
        assert_eq!(s.top_tools.len(), 2);
        assert_eq!(s.top_tools[0].tool_name, "file_read");
        assert_eq!(s.top_tools[0].count, 4);
    }

    #[test]
    fn extract_structured_classifies_as_fetch_url() {
        assert_eq!(
            classify_operation("extract_structured"),
            OperationType::FetchUrl
        );
    }

    #[test]
    fn search_finds_by_tool_name_and_is_user_scoped() {
        let store = test_store();
        // Alice and Bob both have a web_search, but on different sessions
        // and with different user_ids. search() must only return Alice's
        // row when called as Alice.
        store.record(
            "user-alice",
            make_entry("user-alice", "alice-session", "web_search", true),
        )
        .unwrap();
        store.record(
            "user-bob",
            make_entry("user-bob", "bob-session", "web_search", true),
        )
        .unwrap();

        let alice_results = store.search("user-alice", "web_search").unwrap();
        assert_eq!(alice_results.len(), 1);
        assert_eq!(alice_results[0].user_id, "user-alice");

        let bob_results = store.search("user-bob", "web_search").unwrap();
        assert_eq!(bob_results.len(), 1);
        assert_eq!(bob_results[0].user_id, "user-bob");
    }

    #[test]
    fn evicts_oldest_when_over_limit() {
        let store = test_store();
        for i in 0..(MAX_ENTRIES_PER_SESSION + 10) {
            let mut entry = make_entry("user-alice", "evict-test", "file_read", true);
            entry.timestamp = i as i64;
            store.record("user-alice", entry).unwrap();
        }
        let entries = store.list("user-alice", "evict-test").unwrap();
        assert_eq!(entries.len(), MAX_ENTRIES_PER_SESSION);
        // Newest should have the highest timestamp
        assert_eq!(entries[0].timestamp, (MAX_ENTRIES_PER_SESSION + 9) as i64);
        // Oldest kept should be timestamp 10
        assert_eq!(
            entries.last().unwrap().timestamp,
            10
        );
    }

    #[test]
    fn record_overrides_caller_user_id() {
        // The Rust side stamps the user_id from the function argument
        // and ignores whatever the caller put in the entry — a
        // protection against forged attribution.
        let store = test_store();
        let mut forged = make_entry("user-bob", "s1", "file_read", true);
        forged.user_id = "user-alice".to_string();
        store.record("user-bob", forged).unwrap();
        let list = store.list("user-bob", "s1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].user_id, "user-bob");
    }

    #[test]
    fn new_drops_legacy_history_files() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("modeler-history-legacy-{nanos}-{n}"));
        let history = root.join("operation-history");
        std::fs::create_dir_all(&history).unwrap();
        // Pre-existing unscoped history file.
        let legacy = history.join("legacy-session.json");
        std::fs::write(
            &legacy,
            r#"[{"id":"1","user_id":"legacy","session_id":"legacy-session","op_type":"tool_call","tool_name":"file_read","input_preview":"x","success":true,"duration_ms":1,"timestamp":1}]"#,
        )
        .unwrap();

        let _store = OperationHistoryStore::new(root.clone());

        assert!(!legacy.exists(), "legacy history file should be removed");
        assert!(history.is_dir());

        let _ = std::fs::remove_dir_all(&root);
    }
}
