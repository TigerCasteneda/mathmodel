//! Operation History — tracks tool calls with success/failure and timing.
//!
//! Adapted from Claude Code's `memory::history::HistoryManager`, scoped to the
//! Modeler AI app: one JSON file per session under `<app_data>/operation-history/`.
//! Max 500 entries per session (oldest evicted first).

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

/// Persisted operation history store.
///
/// Each session gets its own JSON file.  Reads are cached in memory
/// (load once per lifetime); writes append to the cache + flush to disk.
pub struct OperationHistoryStore {
    history_dir: PathBuf,
    cache: Mutex<HashMap<String, Vec<OperationEntry>>>,
}

impl OperationHistoryStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let history_dir = app_data_dir.join("operation-history");
        let _ = std::fs::create_dir_all(&history_dir);
        Self {
            history_dir,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Ensure the in-memory cache is populated for `session_id`.
    fn ensure_loaded(&self, session_id: &str) -> Result<(), String> {
        let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
        if cache.contains_key(session_id) {
            return Ok(());
        }
        let path = self.session_path(session_id);
        let entries: Vec<OperationEntry> = if path.exists() {
            let content =
                std::fs::read_to_string(&path).map_err(|e| format!("read history: {e}"))?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };
        cache.insert(session_id.to_string(), entries);
        Ok(())
    }

    fn persist(&self, session_id: &str) -> Result<(), String> {
        let cache = self.cache.lock().map_err(|e| e.to_string())?;
        if let Some(entries) = cache.get(session_id) {
            let path = self.session_path(session_id);
            let content = serde_json::to_string_pretty(entries)
                .map_err(|e| format!("serialize history: {e}"))?;
            std::fs::write(&path, &content)
                .map_err(|e| format!("write history: {e}"))?;
        }
        Ok(())
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.history_dir.join(format!("{session_id}.json"))
    }

    /// Record an operation and persist.
    pub fn record(&self, entry: OperationEntry) -> Result<(), String> {
        let session_id = entry.session_id.clone();
        self.ensure_loaded(&session_id)?;
        {
            let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
            let entries = cache
                .get_mut(&session_id)
                .ok_or_else(|| "history cache miss".to_string())?;
            entries.push(entry);
            // FIFO eviction
            while entries.len() > MAX_ENTRIES_PER_SESSION {
                entries.remove(0);
            }
        }
        self.persist(&session_id)
    }

    /// List all operations for a session, newest first.
    pub fn list(&self, session_id: &str) -> Result<Vec<OperationEntry>, String> {
        self.ensure_loaded(session_id)?;
        let cache = self.cache.lock().map_err(|e| e.to_string())?;
        let mut entries = cache
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        entries.reverse(); // newest first
        Ok(entries)
    }

    /// Search all sessions' operations for a query string.
    #[allow(dead_code)]
    pub fn search(&self, query: &str) -> Result<Vec<OperationEntry>, String> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        // Load all session files (not just cached ones)
        let mut results = Vec::new();
        if let Ok(dir_entries) = std::fs::read_dir(&self.history_dir) {
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
    pub fn stats(&self, session_id: &str) -> Result<OperationStats, String> {
        self.ensure_loaded(session_id)?;
        let cache = self.cache.lock().map_err(|e| e.to_string())?;
        let entries = cache.get(session_id).cloned().unwrap_or_default();

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
    pub fn delete_session_history(&self, session_id: &str) -> Result<(), String> {
        let path = self.session_path(session_id);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("delete history: {e}"))?;
        }
        let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
        cache.remove(session_id);
        Ok(())
    }
}

/// Tauri commands ─────────────────────────────────────────────

#[tauri::command]
pub fn list_operations(
    session_id: String,
    store: tauri::State<'_, OperationHistoryStore>,
) -> Result<Vec<OperationEntry>, String> {
    store.list(&session_id)
}

#[tauri::command]
pub fn get_operation_stats(
    session_id: String,
    store: tauri::State<'_, OperationHistoryStore>,
) -> Result<OperationStats, String> {
    store.stats(&session_id)
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

    fn test_store() -> OperationHistoryStore {
        let unique = format!(
            "modeler-history-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        OperationHistoryStore::new(std::env::temp_dir().join(unique))
    }

    fn make_entry(session_id: &str, tool_name: &str, success: bool) -> OperationEntry {
        OperationEntry {
            id: uuid::Uuid::new_v4().to_string(),
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
        store.record(make_entry("s1", "file_read", true)).unwrap();
        store.record(make_entry("s1", "web_search", true)).unwrap();
        store.record(make_entry("s1", "execute_command", false)).unwrap();
        store.record(make_entry("s2", "fetch_url", true)).unwrap();

        let s1 = store.list("s1").unwrap();
        assert_eq!(s1.len(), 3, "s1 should have 3 entries (newest first)");

        let s2 = store.list("s2").unwrap();
        assert_eq!(s2.len(), 1);
    }

    #[test]
    fn stats_computes_correctly() {
        let store = test_store();
        for _ in 0..4 {
            store.record(make_entry("stats-session", "file_read", true)).unwrap();
        }
        store.record(make_entry("stats-session", "execute_command", false)).unwrap();

        let s = store.stats("stats-session").unwrap();
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
    fn search_finds_by_tool_name() {
        let store = test_store();
        store.record(make_entry("search-test", "web_search", true)).unwrap();
        store.record(make_entry("search-test", "file_read", true)).unwrap();

        let results = store.search("web_search").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_name, "web_search");
    }

    #[test]
    fn evicts_oldest_when_over_limit() {
        let store = test_store();
        for i in 0..(MAX_ENTRIES_PER_SESSION + 10) {
            let mut entry = make_entry("evict-test", "file_read", true);
            entry.timestamp = i as i64;
            store.record(entry).unwrap();
        }
        let entries = store.list("evict-test").unwrap();
        assert_eq!(entries.len(), MAX_ENTRIES_PER_SESSION);
        // Newest should have the highest timestamp
        assert_eq!(entries[0].timestamp, (MAX_ENTRIES_PER_SESSION + 9) as i64);
        // Oldest kept should be timestamp 10
        assert_eq!(
            entries.last().unwrap().timestamp,
            10
        );
    }
}
