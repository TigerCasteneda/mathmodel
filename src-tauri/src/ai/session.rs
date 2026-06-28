use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Archived,
}

impl Default for SessionStatus {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    pub timestamp: i64,
    #[serde(default)]
    pub tool_calls: Option<Vec<claude_code_rs::api::ToolCall>>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
    #[serde(default)]
    pub status: SessionStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub message_count: usize,
    pub status: SessionStatus,
}

/// Persisted session store backed by JSON files in the Tauri app data dir.
/// Layout is `chat-sessions/{user_id}/{conversation_id}.json` so that two
/// accounts logging into the same installation cannot see each other's
/// history (previously everything shared the root, which leaked across
/// accounts).
pub struct ChatSessionStore {
    /// Root directory that holds per-user subdirectories.
    sessions_dir: PathBuf,
    /// In-memory cache keyed by conversation_id. Holds the currently
    /// active user's sessions; cleared on logout by the frontend which
    /// issues a fresh `load_session` for each tab it cares about.
    active: Mutex<HashMap<String, Session>>,
}

/// Strip a Supabase-style user id down to a safe filesystem component.
/// The id is normally a UUID (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`),
/// but we don't trust the caller — anything that isn't alphanumeric,
/// `-`, or `_` is dropped. An empty result falls back to `"unknown"` so
/// paths never silently produce empty directory names.
fn sanitize_user_id(user_id: &str) -> String {
    let cleaned: String = user_id
        .chars()
        .filter(|&c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

impl ChatSessionStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let sessions_dir = app_data_dir.join("chat-sessions");
        let _ = std::fs::create_dir_all(&sessions_dir);

        // One-shot migration: any loose *.json files sitting at the root
        // were written by the previous shared-store implementation and are
        // now invisible to the per-user list/load paths. Drop them so the
        // directory reflects the new layout and we don't accidentally
        // surface stale data when a user_id is unknown. Safe to run on
        // every startup — once migrated it's a no-op.
        if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_dir = path
                    .metadata()
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
                let is_json = path.extension().and_then(|e| e.to_str()) == Some("json");
                if !is_dir && is_json {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }

        Self {
            sessions_dir,
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Resolve the on-disk directory that holds a user's sessions. Creates
    /// it lazily on first write so empty accounts don't litter the disk.
    fn user_dir(&self, user_id: &str) -> PathBuf {
        self.sessions_dir.join(sanitize_user_id(user_id))
    }

    fn session_path(&self, user_id: &str, conversation_id: &str) -> PathBuf {
        self.user_dir(user_id)
            .join(format!("{conversation_id}.json"))
    }

    /// List all saved session infos (metadata only, no messages).
    pub fn list(&self, user_id: &str) -> Result<Vec<SessionInfo>, String> {
        let mut list = Vec::new();
        let user_dir = self.user_dir(user_id);
        if let Ok(entries) = std::fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(session) = serde_json::from_str::<Session>(&content) {
                            let status = session.status.clone();
                            list.push(SessionInfo {
                                id: session.id,
                                name: session.name,
                                created_at: session.created_at,
                                message_count: session.messages.len(),
                                status,
                            });
                        }
                    }
                }
            }
        }
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(list)
    }

    /// Load or create a session by id.
    pub fn load(&self, user_id: &str, conversation_id: &str) -> Result<Session, String> {
        {
            let active = self.active.lock().map_err(|e| e.to_string())?;
            if let Some(session) = active.get(conversation_id) {
                return Ok(session.clone());
            }
        }

        let path = self.session_path(user_id, conversation_id);
        let session = if path.exists() {
            let content =
                std::fs::read_to_string(&path).map_err(|e| format!("read session: {e}"))?;
            serde_json::from_str::<Session>(&content).map_err(|e| format!("parse session: {e}"))?
        } else {
            Session {
                id: conversation_id.to_string(),
                name: "New Chat".to_string(),
                created_at: chrono::Utc::now().timestamp(),
                updated_at: chrono::Utc::now().timestamp(),
                messages: Vec::new(),
                status: SessionStatus::Active,
            }
        };

        let mut active = self.active.lock().map_err(|e| e.to_string())?;
        active.insert(conversation_id.to_string(), session.clone());
        Ok(session)
    }

    /// Persist a session to disk and update in-memory.
    fn persist(&self, user_id: &str, session: &Session) -> Result<(), String> {
        let user_dir = self.user_dir(user_id);
        std::fs::create_dir_all(&user_dir)
            .map_err(|e| format!("create user dir: {e:#}"))?;
        let path = user_dir.join(format!("{}.json", session.id));
        let content =
            serde_json::to_string_pretty(session).map_err(|e| format!("serialize session: {e}"))?;
        std::fs::write(&path, &content).map_err(|e| format!("write session: {e}"))?;

        let mut active = self.active.lock().map_err(|e| e.to_string())?;
        active.insert(session.id.clone(), session.clone());
        Ok(())
    }

    pub fn push_chat_message(
        &self,
        user_id: &str,
        conversation_id: &str,
        message: claude_code_rs::api::ChatMessage,
    ) -> Result<(), String> {
        let mut session = self.load(user_id, conversation_id)?;
        if session.name == "New Chat" && message.role == "user" {
            if let Some(content) = message.content.as_deref() {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    session.name = trimmed.chars().take(50).collect();
                }
            }
        }
        session.messages.push(SessionMessage {
            role: message.role,
            content: message.content,
            timestamp: chrono::Utc::now().timestamp(),
            tool_calls: message.tool_calls,
            tool_call_id: message.tool_call_id,
        });
        session.updated_at = chrono::Utc::now().timestamp();
        self.persist(user_id, &session)
    }

    /// Push a user message and persist.
    pub fn push_user(
        &self,
        user_id: &str,
        conversation_id: &str,
        content: String,
    ) -> Result<(), String> {
        self.push_chat_message(
            user_id,
            conversation_id,
            claude_code_rs::api::ChatMessage::user(content),
        )
    }

    /// Push an assistant message and persist.
    pub fn push_assistant(
        &self,
        user_id: &str,
        conversation_id: &str,
        content: String,
    ) -> Result<(), String> {
        self.push_chat_message(
            user_id,
            conversation_id,
            claude_code_rs::api::ChatMessage::assistant(content),
        )
    }

    /// Return Vec<ChatMessage> suitable for the API call.
    pub fn history(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Vec<claude_code_rs::api::ChatMessage>, String> {
        let session = self.load(user_id, conversation_id)?;
        Ok(session
            .messages
            .iter()
            .map(|m| claude_code_rs::api::ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                tool_calls: m.tool_calls.clone(),
                tool_call_id: m.tool_call_id.clone(),
            })
            .collect())
    }

    pub fn history_with_timestamps(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Vec<super::compaction::ContextMessage>, String> {
        let session = self.load(user_id, conversation_id)?;
        Ok(session
            .messages
            .iter()
            .map(|message| super::compaction::ContextMessage {
                message: claude_code_rs::api::ChatMessage {
                    role: message.role.clone(),
                    content: message.content.clone(),
                    tool_calls: message.tool_calls.clone(),
                    tool_call_id: message.tool_call_id.clone(),
                },
                timestamp: message.timestamp,
            })
            .collect())
    }

    pub fn delete(&self, user_id: &str, conversation_id: &str) -> Result<(), String> {
        let path = self.session_path(user_id, conversation_id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("delete session: {e}"))?;
        }
        let mut active = self.active.lock().map_err(|e| e.to_string())?;
        active.remove(conversation_id);
        Ok(())
    }

    /// Rename a session.
    pub fn rename(
        &self,
        user_id: &str,
        conversation_id: &str,
        new_name: &str,
    ) -> Result<(), String> {
        let mut session = self.load(user_id, conversation_id)?;
        session.name = new_name.trim().chars().take(100).collect();
        session.updated_at = chrono::Utc::now().timestamp();
        self.persist(user_id, &session)
    }

    /// Archive a session (sets status to Archived).
    pub fn archive(&self, user_id: &str, conversation_id: &str) -> Result<(), String> {
        let mut session = self.load(user_id, conversation_id)?;
        session.status = SessionStatus::Archived;
        session.updated_at = chrono::Utc::now().timestamp();
        self.persist(user_id, &session)
    }

    /// Unarchive a session (sets status back to Active).
    pub fn unarchive(&self, user_id: &str, conversation_id: &str) -> Result<(), String> {
        let mut session = self.load(user_id, conversation_id)?;
        session.status = SessionStatus::Active;
        session.updated_at = chrono::Utc::now().timestamp();
        self.persist(user_id, &session)
    }

    /// Search sessions by name and message content.
    /// Returns matching SessionInfo, scanning at most the first 2KB of each
    /// message body for performance.
    pub fn search(&self, user_id: &str, query: &str) -> Result<Vec<SessionInfo>, String> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return self.list(user_id);
        }
        let all = self.list(user_id)?;
        Ok(all
            .into_iter()
            .filter(|info| {
                if info.name.to_lowercase().contains(&query) {
                    return true;
                }
                let path = self.session_path(user_id, &info.id);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let search_buf: String = content.chars().take(8_192).collect();
                    search_buf.to_lowercase().contains(&query)
                } else {
                    false
                }
            })
            .collect())
    }

    /// Move the in-memory cache aside so a fresh user won't briefly see
    /// another user's sessions. Currently a no-op because the frontend
    /// always issues a `load_session` per tab on identity change; kept
    /// here as the seam if we ever cache sessions across tabs.
    #[allow(dead_code)]
    pub fn clear_active_cache(&self) {
        if let Ok(mut active) = self.active.lock() {
            active.clear();
        }
    }

    #[cfg(test)]
    fn sessions_dir_for_tests(&self) -> &PathBuf {
        &self.sessions_dir
    }

    #[cfg(test)]
    fn user_dir_for_tests(&self, user_id: &str) -> PathBuf {
        self.user_dir(user_id)
    }
}

/// ── Tauri commands ──
///
/// Every command takes `user_id` so the store can scope reads and writes
/// to `chat-sessions/{user_id}/`. The frontend pulls `user_id` out of
/// `useAuth()` (which decodes it from the Supabase JWT) and threads it
/// through `lib/tauri-api.ts` wrappers. Without this, anyone who logs
/// in sees every other account's history on the same machine.

#[tauri::command]
pub fn list_sessions(
    user_id: String,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<Vec<SessionInfo>, String> {
    store.list(&user_id)
}

#[tauri::command]
pub fn load_session(
    user_id: String,
    conversation_id: Option<String>,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<Session, String> {
    store.load(&user_id, &conversation_id.unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
pub fn delete_session(
    user_id: String,
    conversation_id: String,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<(), String> {
    store.delete(&user_id, &conversation_id)
}

#[tauri::command]
pub fn rename_session(
    user_id: String,
    conversation_id: String,
    new_name: String,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<(), String> {
    store.rename(&user_id, &conversation_id, &new_name)
}

#[tauri::command]
pub fn archive_session(
    user_id: String,
    conversation_id: String,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<(), String> {
    store.archive(&user_id, &conversation_id)
}

#[tauri::command]
pub fn unarchive_session(
    user_id: String,
    conversation_id: String,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<(), String> {
    store.unarchive(&user_id, &conversation_id)
}

#[tauri::command]
pub fn search_sessions(
    user_id: String,
    query: String,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<Vec<SessionInfo>, String> {
    store.search(&user_id, &query)
}

#[tauri::command]
pub fn export_session(
    user_id: String,
    conversation_id: String,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<Vec<claude_code_rs::api::ChatMessage>, String> {
    store.history(&user_id, &conversation_id)
}

impl Default for ChatSessionStore {
    fn default() -> Self {
        Self::new(PathBuf::from("data/chat-sessions"))
    }
}

#[cfg(test)]
mod tests {
    use super::{sanitize_user_id, ChatSessionStore};

    fn test_store() -> ChatSessionStore {
        let unique = format!(
            "modeler-session-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        ChatSessionStore::new(std::env::temp_dir().join(unique))
    }

    #[test]
    fn sanitize_user_id_keeps_uuid_like_strings_intact() {
        let id = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
        assert_eq!(sanitize_user_id(id), id);
    }

    #[test]
    fn sanitize_user_id_drops_path_separators_and_shells() {
        // Path traversal attempts become harmless suffixes. The filter keeps
        // only `[A-Za-z0-9_-]`, so `;`, spaces, and `/` all vanish while
        // `-` survives (used in UUIDs). The test inputs each contain at most
        // one `-` — the assertion matches what stays after stripping.
        assert_eq!(sanitize_user_id("../../etc/passwd"), "etcpasswd");
        // Input has only one `-` between "rm" and "rf"; nothing between
        // "user" and "rm", so the output concatenates without that gap.
        assert_eq!(sanitize_user_id("user; rm -rf /"), "userrm-rf");
        assert_eq!(sanitize_user_id(""), "unknown");
        assert_eq!(sanitize_user_id("/../foo"), "foo");
        assert_eq!(sanitize_user_id("a-b-c"), "a-b-c");
    }

    #[test]
    fn sanitize_user_id_truncates_very_long_inputs() {
        let long = "a".repeat(200);
        let out = sanitize_user_id(&long);
        assert_eq!(out.len(), 64);
    }

    #[test]
    fn sessions_are_isolated_per_user_id() {
        let store = test_store();
        let alice = "user-alice";
        let bob = "user-bob";

        store
            .push_user(alice, "alice-chat-1", "alice's secret plan".into())
            .unwrap();
        store
            .push_user(bob, "bob-chat-1", "bob's secret plan".into())
            .unwrap();

        let alice_list = store.list(alice).unwrap();
        let bob_list = store.list(bob).unwrap();

        assert_eq!(alice_list.len(), 1);
        assert_eq!(bob_list.len(), 1);
        assert_eq!(alice_list[0].name, "alice's secret plan");
        assert_eq!(bob_list[0].name, "bob's secret plan");

        // Disk layout puts alice and bob in distinct directories.
        assert!(store.user_dir_for_tests(alice).join("alice-chat-1.json").exists());
        assert!(store.user_dir_for_tests(bob).join("bob-chat-1.json").exists());
        assert!(!store.user_dir_for_tests(alice).join("bob-chat-1.json").exists());
        assert!(!store.user_dir_for_tests(bob).join("alice-chat-1.json").exists());

        let _ = std::fs::remove_dir_all(store.sessions_dir_for_tests());
    }

    #[test]
    fn history_preserves_tool_calls_and_tool_call_id() {
        let store = test_store();
        let user = "history-user";
        let conversation_id = "tool-history";

        store
            .push_chat_message(
                user,
                conversation_id,
                claude_code_rs::api::ChatMessage::assistant_with_tools(vec![
                    claude_code_rs::api::ToolCall {
                        id: "call_1".to_string(),
                        r#type: "function".to_string(),
                        function: claude_code_rs::api::ToolCallFunction {
                            name: "web_search".to_string(),
                            arguments: r#"{"query":"sir"}"#.to_string(),
                        },
                    },
                ]),
            )
            .unwrap();
        store
            .push_chat_message(
                user,
                conversation_id,
                claude_code_rs::api::ChatMessage::tool("call_1", r#"{"success":true}"#),
            )
            .unwrap();

        let history = store.history(user, conversation_id).unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(
            history[0].tool_calls.as_ref().map(|calls| calls.len()),
            Some(1)
        );
        assert_eq!(history[1].tool_call_id.as_deref(), Some("call_1"));

        let _ = std::fs::remove_dir_all(store.sessions_dir_for_tests());
    }

    #[test]
    fn first_user_message_sets_session_title() {
        let store = test_store();
        let user = "title-user";
        let conversation_id = "title";

        store
            .push_user(
                user,
                conversation_id,
                "Build a traffic prediction baseline with graph neural networks".to_string(),
            )
            .unwrap();

        let session = store.load(user, conversation_id).unwrap();
        assert_eq!(
            session.name,
            "Build a traffic prediction baseline with graph neu"
        );

        let _ = std::fs::remove_dir_all(store.sessions_dir_for_tests());
    }

    #[test]
    fn legacy_string_content_session_still_loads() {
        let store = test_store();
        let user = "legacy-user";
        let session_path = store.user_dir_for_tests(user).join("legacy.json");
        std::fs::create_dir_all(session_path.parent().unwrap()).unwrap();
        std::fs::write(
            &session_path,
            r#"{"id":"legacy","name":"New Chat","created_at":1,"updated_at":1,"messages":[{"role":"assistant","content":"hello","timestamp":1}]}"#,
        )
        .unwrap();

        let history = store.history(user, "legacy").unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content.as_deref(), Some("hello"));

        let _ = std::fs::remove_dir_all(store.sessions_dir_for_tests());
    }

    #[test]
    fn new_store_drops_loose_root_files_but_keeps_user_dirs() {
        // Simulate the post-fix layout: a user dir with a real session,
        // plus a stray loose file at the chat-sessions root from the old
        // shared-store implementation. Constructor should drop the loose
        // file and leave the user dir untouched.
        let root = std::env::temp_dir().join(format!(
            "modeler-session-migrate-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        // The store's sessions_dir is `root/chat-sessions/`. Stage files
        // inside that boundary so the migration loop actually sees them.
        let chat_sessions = root.join("chat-sessions");
        std::fs::create_dir_all(&chat_sessions).unwrap();

        // Pre-existing user content.
        let user_dir = chat_sessions.join("user-alice");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(
            user_dir.join("conv-1.json"),
            r#"{"id":"conv-1","name":"kept","created_at":1,"updated_at":1,"messages":[]}"#,
        )
        .unwrap();

        // Stray loose file the old code would have written here.
        std::fs::write(
            chat_sessions.join("orphan.json"),
            r#"{"id":"orphan","name":"Old mixed-user chat","created_at":1,"updated_at":1,"messages":[]}"#,
        )
        .unwrap();

        let _store = ChatSessionStore::new(root.clone());

        assert!(
            !chat_sessions.join("orphan.json").exists(),
            "loose root file should be removed on startup"
        );
        assert!(
            user_dir.join("conv-1.json").exists(),
            "user-dir content must survive migration"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}

// Suppress unused-import warning for `Path` if no test in this module
// currently uses it directly (kept available for future helpers).
#[allow(dead_code)]
fn _path_marker(_p: &Path) {}