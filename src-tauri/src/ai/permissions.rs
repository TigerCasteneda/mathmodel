use crate::ai::runtime::PermissionMode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;
use tokio::sync::oneshot;

const MAX_CONSECUTIVE_DENIALS: usize = 3;
const MAX_TOTAL_DENIALS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PermissionConfig {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub deny_list: Vec<String>,
    #[serde(default)]
    pub ask_list: Vec<String>,
    #[serde(default)]
    pub allow_list: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRule {
    pub tool_name: String,
    pub rule_content: Option<String>,
}

impl PermissionRule {
    pub fn parse(value: &str) -> Result<Self, String> {
        let open = find_first_unescaped_char(value, '(');
        if open.is_none() {
            return Ok(Self {
                tool_name: normalize_tool_name(value),
                rule_content: None,
            });
        }

        let open = open.unwrap_or_default();
        let close = find_last_unescaped_char(value, ')');
        if close.is_none() || close.unwrap_or_default() <= open || close != Some(value.len() - 1) {
            return Ok(Self {
                tool_name: normalize_tool_name(value),
                rule_content: None,
            });
        }

        let tool_name = &value[..open];
        if tool_name.trim().is_empty() {
            return Err("permission rule tool name is required".to_string());
        }

        let raw_content = &value[open + 1..close.unwrap_or_default()];
        let rule_content = if raw_content.is_empty() || raw_content == "*" {
            None
        } else {
            Some(unescape_rule_content(raw_content))
        };

        Ok(Self {
            tool_name: normalize_tool_name(tool_name),
            rule_content,
        })
    }

    fn matches(&self, request: &PermissionRequest) -> bool {
        if !self.tool_name.eq_ignore_ascii_case(&request.tool_name) {
            return false;
        }

        match (&self.rule_content, &request.content) {
            (None, _) => true,
            (Some(pattern), Some(content)) => wildcard_match(pattern, content),
            (Some(_), None) => false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DenialTracker {
    pub consecutive_denials: usize,
    pub total_denials: usize,
}

impl DenialTracker {
    fn record_denial(&self) -> Self {
        Self {
            consecutive_denials: self.consecutive_denials + 1,
            total_denials: self.total_denials + 1,
        }
    }

    fn reset(&self) -> Self {
        Self::default()
    }

    fn is_locked(&self) -> bool {
        self.consecutive_denials >= MAX_CONSECUTIVE_DENIALS
            || self.total_denials >= MAX_TOTAL_DENIALS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone)]
pub struct PermissionOutcome {
    pub decision: PermissionDecision,
    pub reason: String,
    pub next_tracker: DenialTracker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPromptRequest {
    pub request_id: String,
    pub conversation_id: String,
    pub tool_name: String,
    pub arguments: Value,
    pub reason: String,
    pub mode: String,
    pub content: Option<String>,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionAction {
    ReadOnly,
    Edit,
    Command,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub content: Option<String>,
    action: PermissionAction,
}

impl PermissionRequest {
    pub fn from_tool_call(tool_name: &str, arguments: &Value) -> Self {
        let tool_name = normalize_tool_name(tool_name);
        let action = tool_action(&tool_name);
        let content = request_content(&tool_name, arguments);

        Self {
            tool_name,
            content,
            action,
        }
    }
}

/// Per-user permission store. The previous implementation held a single
/// `Arc<Mutex<PermissionState>>` (config + DenialTracker) plus a single
/// `HashMap<String, oneshot::Sender<bool>>` for pending prompts, so
/// every account on the same desktop install shared:
///  - the deny/allow/ask lists (User B's tool calls were evaluated
///    against User A's rules)
///  - the consecutive/total denial counter (User B inherited User A's
///    lockouts)
///  - the pending prompt map (a `request_id` for User A's prompt
///    could be resolved by User B)
/// Now state and pending are both keyed by `user_id`.
#[derive(Clone)]
pub struct PermissionStore {
    /// Directory that holds per-user `<sanitized_user_id>.json` files.
    config_dir: PathBuf,
    /// user_id -> { config, tracker }. Lazy-loaded on first access.
    state: Arc<Mutex<HashMap<String, PermissionState>>>,
    /// user_id -> request_id -> oneshot sender. The outer map is
    /// `Mutex<HashMap<...>>`; the inner one only needs to be unique per
    /// user so we use `Mutex` rather than `RwLock` to keep the simple
    /// lock ordering (outer first, then inner if needed).
    pending: Arc<Mutex<HashMap<String, HashMap<String, oneshot::Sender<bool>>>>>,
}

#[derive(Debug, Clone)]
struct PermissionState {
    config: PermissionConfig,
    tracker: DenialTracker,
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

fn user_config_path(config_dir: &PathBuf, user_id: &str) -> PathBuf {
    config_dir.join(format!("{}.json", sanitize_user_id(user_id)))
}

impl PermissionStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let config_dir = app_data_dir.join("permissions");
        let _ = std::fs::create_dir_all(&config_dir);

        // One-shot migration: the previous layout stored a single
        // `permissions.json` at the data-dir root. We can't safely
        // attribute its deny/allow lists to any user_id, so drop it.
        // Consistent with chat / hooks / plans / plugins migration
        // policy: user chose 'delete old data'.
        let legacy = app_data_dir.join("permissions.json");
        if legacy.exists() {
            let _ = std::fs::remove_file(&legacy);
        }

        Self {
            config_dir,
            state: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Read the user's config from disk, populating the in-memory cache
    /// on first access. Returns an empty default if the file is absent
    /// or unparseable.
    fn load_user_state(&self, user_id: &str) -> PermissionState {
        let path = user_config_path(&self.config_dir, user_id);
        let config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<PermissionConfig>(&content).ok())
            .unwrap_or_default();
        PermissionState {
            config,
            tracker: DenialTracker::default(),
        }
    }

    fn get_or_init_user_state(&self, user_id: &str) -> Result<PermissionState, String> {
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        if let Some(s) = state.get(user_id) {
            return Ok(s.clone());
        }
        let new_state = self.load_user_state(user_id);
        state.insert(user_id.to_string(), new_state.clone());
        Ok(new_state)
    }

    pub fn get_config(&self, user_id: &str) -> Result<PermissionConfig, String> {
        let state = self.get_or_init_user_state(user_id)?;
        Ok(state.config)
    }

    pub fn set_config(
        &self,
        user_id: &str,
        config: PermissionConfig,
    ) -> Result<PermissionConfig, String> {
        validate_mode_value(config.mode.as_deref())?;
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        // Force lazy-load first so we don't overwrite a previously
        // persisted config with an empty default.
        let new_state = match state.get(user_id) {
            Some(_) => PermissionState {
                config: config.clone(),
                tracker: state
                    .get(user_id)
                    .map(|s| s.tracker.clone())
                    .unwrap_or_default(),
            },
            None => {
                let loaded = self.load_user_state(user_id);
                PermissionState {
                    config: config.clone(),
                    tracker: loaded.tracker,
                }
            }
        };
        state.insert(user_id.to_string(), new_state);
        persist_permission_config(&user_config_path(&self.config_dir, user_id), &config)?;
        Ok(config)
    }

    pub fn configured_mode(&self, user_id: &str) -> Result<Option<String>, String> {
        let state = self.get_or_init_user_state(user_id)?;
        Ok(state.config.mode.clone())
    }

    pub fn evaluate_tool_call(
        &self,
        user_id: &str,
        mode: PermissionMode,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<PermissionOutcome, String> {
        let request = PermissionRequest::from_tool_call(tool_name, arguments);
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        let user_state = state
            .entry(user_id.to_string())
            .or_insert_with(|| self.load_user_state(user_id));
        let outcome = evaluate_permission(
            &user_state.config,
            user_state.tracker.clone(),
            mode,
            &request,
        );
        user_state.tracker = outcome.next_tracker.clone();
        Ok(outcome)
    }

    pub fn wait_for_resolution(
        &self,
        user_id: &str,
        request: PermissionPromptRequest,
        timeout: std::time::Duration,
    ) -> impl Future<Output = Result<bool, String>> + Send + 'static {
        let (sender, receiver) = oneshot::channel();
        let pending = self.pending.clone();
        {
            let mut outer = pending.lock().expect("permission pending outer lock");
            let user_map = outer.entry(user_id.to_string()).or_default();
            user_map.insert(request.request_id.clone(), sender);
        }
        let user_id_owned = user_id.to_string();
        async move {
            let resolved = match tokio::time::timeout(timeout, receiver).await {
                Ok(Ok(allow)) => allow,
                Ok(Err(_)) => false,
                Err(_) => false,
            };
            let mut outer = pending.lock().map_err(|error| error.to_string())?;
            if let Some(user_map) = outer.get_mut(&user_id_owned) {
                user_map.remove(&request.request_id);
            }
            Ok(resolved)
        }
    }

    /// Resolve a pending permission request. Restricted to the
    /// requesting user: a request_id is scoped by user_id, so
    /// User B cannot answer User A's prompt (even if they could
    /// somehow learn the request_id, which is a UUID v4).
    pub fn resolve_request(
        &self,
        user_id: &str,
        request_id: &str,
        allow: bool,
    ) -> Result<(), String> {
        let sender = {
            let mut outer = self.pending.lock().map_err(|error| error.to_string())?;
            let user_map = outer
                .get_mut(user_id)
                .ok_or_else(|| format!("unknown permission request: {request_id}"))?;
            user_map.remove(request_id)
        }
        .ok_or_else(|| format!("unknown permission request: {request_id}"))?;

        sender
            .send(allow)
            .map_err(|_| format!("permission request already closed: {request_id}"))
    }
}

pub fn evaluate_permission(
    config: &PermissionConfig,
    tracker: DenialTracker,
    mode: PermissionMode,
    request: &PermissionRequest,
) -> PermissionOutcome {
    if matches_rule_list(&config.deny_list, request) {
        return deny_outcome(
            tracker,
            format!("{} was denied by permission rules.", request.tool_name),
        );
    }

    if matches_rule_list(&config.ask_list, request) {
        return ask_outcome(
            tracker,
            format!(
                "{} requires approval by permission rules. Interactive approval is not wired yet.",
                request.tool_name
            ),
        );
    }

    let default_decision = match request.action {
        PermissionAction::ReadOnly | PermissionAction::Other => PermissionDecision::Allow,
        PermissionAction::Edit => {
            if matches!(
                mode,
                PermissionMode::AcceptEdit | PermissionMode::Auto | PermissionMode::Bypass
            ) {
                PermissionDecision::Allow
            } else {
                PermissionDecision::Deny
            }
        }
        PermissionAction::Command => {
            let command = request.content.as_deref().unwrap_or_default();
            match mode {
                PermissionMode::Bypass => PermissionDecision::Allow,
                PermissionMode::Auto if is_low_risk_command(command) => PermissionDecision::Allow,
                _ => PermissionDecision::Deny,
            }
        }
    };

    let final_decision = if matches!(default_decision, PermissionDecision::Deny)
        && matches_rule_list(&config.allow_list, request)
    {
        PermissionDecision::Allow
    } else {
        default_decision
    };

    match final_decision {
        PermissionDecision::Allow => allow_outcome(tracker, allowed_reason(mode, request)),
        PermissionDecision::Deny if tracker.is_locked() => ask_outcome(
            tracker,
            format!(
                "{} hit the permission circuit breaker after repeated denials. Interactive approval is not wired yet.",
                request.tool_name
            ),
        ),
        PermissionDecision::Deny => deny_outcome(
            tracker,
            format!(
                "{} requires a broader permission mode. Current mode is {}.",
                request.tool_name,
                mode.label()
            ),
        ),
        PermissionDecision::Ask => ask_outcome(
            tracker,
            format!(
                "{} requires approval. Interactive approval is not wired yet.",
                request.tool_name
            ),
        ),
    }
}

#[tauri::command]
pub fn get_permission_config(
    user_id: String,
    store: State<'_, PermissionStore>,
) -> Result<PermissionConfig, String> {
    store.get_config(&user_id)
}

#[tauri::command]
pub fn set_permission_config(
    user_id: String,
    config: PermissionConfig,
    store: State<'_, PermissionStore>,
) -> Result<PermissionConfig, String> {
    store.set_config(&user_id, config)
}

#[tauri::command]
pub fn resolve_permission_request(
    user_id: String,
    request_id: String,
    allow: bool,
    store: State<'_, PermissionStore>,
) -> Result<(), String> {
    store.resolve_request(&user_id, &request_id, allow)
}

fn persist_permission_config(path: &PathBuf, config: &PermissionConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create permission dir: {error}"))?;
    }
    let content = serde_json::to_string_pretty(config)
        .map_err(|error| format!("serialize permission config: {error}"))?;
    std::fs::write(path, content).map_err(|error| format!("write permission config: {error}"))
}

fn validate_mode_value(value: Option<&str>) -> Result<(), String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(()),
        Some("default" | "accept_edit" | "auto" | "bypass") => Ok(()),
        Some(other) => Err(format!("unsupported permission mode: {other}")),
    }
}

fn matches_rule_list(rules: &[String], request: &PermissionRequest) -> bool {
    rules.iter().any(|rule| {
        PermissionRule::parse(rule)
            .map(|parsed| parsed.matches(request))
            .unwrap_or(false)
    })
}

fn allow_outcome(tracker: DenialTracker, reason: String) -> PermissionOutcome {
    PermissionOutcome {
        decision: PermissionDecision::Allow,
        reason,
        next_tracker: tracker.reset(),
    }
}

fn deny_outcome(tracker: DenialTracker, reason: String) -> PermissionOutcome {
    PermissionOutcome {
        decision: PermissionDecision::Deny,
        reason,
        next_tracker: tracker.record_denial(),
    }
}

fn ask_outcome(tracker: DenialTracker, reason: String) -> PermissionOutcome {
    PermissionOutcome {
        decision: PermissionDecision::Ask,
        reason,
        next_tracker: tracker.reset(),
    }
}

fn allowed_reason(mode: PermissionMode, request: &PermissionRequest) -> String {
    match request.action {
        PermissionAction::Command => format!(
            "{} was allowed under {} mode.",
            request.tool_name,
            mode.label()
        ),
        PermissionAction::Edit => format!(
            "{} was allowed under {} mode.",
            request.tool_name,
            mode.label()
        ),
        _ => format!("{} is allowed.", request.tool_name),
    }
}

fn tool_action(tool_name: &str) -> PermissionAction {
    match tool_name {
        "file_write" | "write_file" | "file_edit" | "save_reference" => PermissionAction::Edit,
        "execute_command" => PermissionAction::Command,
        "tool_search" | "file_read" | "read_file" | "list_files" | "search_files"
        | "web_search" | "fetch_url" => PermissionAction::ReadOnly,
        _ => PermissionAction::Other,
    }
}

fn request_content(tool_name: &str, arguments: &Value) -> Option<String> {
    let value = match tool_name {
        "execute_command" => arguments.get("command").and_then(Value::as_str),
        "file_write" | "write_file" | "file_edit" => arguments
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| arguments.get("file_path").and_then(Value::as_str)),
        "save_reference" => arguments
            .get("title")
            .and_then(Value::as_str)
            .or_else(|| arguments.get("url").and_then(Value::as_str)),
        "file_read" | "read_file" => arguments
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| arguments.get("file_path").and_then(Value::as_str)),
        "fetch_url" => arguments.get("url").and_then(Value::as_str),
        "web_search" | "tool_search" => arguments.get("query").and_then(Value::as_str),
        "search_files" => arguments.get("pattern").and_then(Value::as_str),
        "start_background_task" => arguments.get("prompt").and_then(Value::as_str),
        _ => None,
    };

    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_tool_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "bash" => "execute_command".to_string(),
        "read" => "file_read".to_string(),
        "write" => "file_write".to_string(),
        other => other.to_string(),
    }
}

fn is_low_risk_command(command: &str) -> bool {
    let trimmed = command.trim().to_lowercase();
    let blocked = [
        "rm ",
        "del ",
        "rmdir",
        "git reset",
        "git clean",
        "shutdown",
        "format ",
    ];
    if blocked
        .iter()
        .any(|prefix| trimmed.starts_with(prefix) || trimmed.contains(&format!("&& {prefix}")))
    {
        return false;
    }

    [
        "dir",
        "ls",
        "pwd",
        "git status",
        "git diff",
        "npm test",
        "npm run test",
        "cargo test",
        "cargo check",
        "npx tsc",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

fn unescape_rule_content(content: &str) -> String {
    content
        .replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\\\", "\\")
}

fn find_first_unescaped_char(value: &str, needle: char) -> Option<usize> {
    for (index, current) in value.char_indices() {
        if current != needle {
            continue;
        }
        let mut backslashes = 0usize;
        for previous in value[..index].chars().rev() {
            if previous == '\\' {
                backslashes += 1;
            } else {
                break;
            }
        }
        if backslashes % 2 == 0 {
            return Some(index);
        }
    }
    None
}

fn find_last_unescaped_char(value: &str, needle: char) -> Option<usize> {
    for (index, current) in value.char_indices().rev() {
        if current != needle {
            continue;
        }
        let mut backslashes = 0usize;
        for previous in value[..index].chars().rev() {
            if previous == '\\' {
                backslashes += 1;
            } else {
                break;
            }
        }
        if backslashes % 2 == 0 {
            return Some(index);
        }
    }
    None
}

fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let pattern = pattern.to_ascii_lowercase();
    let candidate = candidate.to_ascii_lowercase();
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == candidate;
    }

    let mut remainder = candidate.as_str();
    let starts_with_wildcard = pattern.starts_with('*');
    let ends_with_wildcard = pattern.ends_with('*');

    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if index == 0 && !starts_with_wildcard {
            if !remainder.starts_with(part) {
                return false;
            }
            remainder = &remainder[part.len()..];
            continue;
        }

        if index == parts.len() - 1 && !ends_with_wildcard {
            return remainder.ends_with(part);
        }

        if let Some(found) = remainder.find(part) {
            remainder = &remainder[found + part.len()..];
        } else {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::ai::runtime::PermissionMode;

    fn unique_tmp_dir(label: &str) -> std::path::PathBuf {
        let nanos = chrono::Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!("modeler-perms-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_rule_with_tool_alias_and_wildcard_content() {
        let rule = super::PermissionRule::parse("Bash(git status *)").unwrap();

        assert_eq!(rule.tool_name, "execute_command");
        assert_eq!(rule.rule_content.as_deref(), Some("git status *"));
    }

    #[test]
    fn auto_mode_allows_low_risk_command_but_accept_edit_does_not() {
        let config = super::PermissionConfig::default();
        let request = super::PermissionRequest::from_tool_call(
            "execute_command",
            &serde_json::json!({ "command": "git status" }),
        );

        let accept_edit = super::evaluate_permission(
            &config,
            super::DenialTracker::default(),
            PermissionMode::AcceptEdit,
            &request,
        );
        let auto = super::evaluate_permission(
            &config,
            super::DenialTracker::default(),
            PermissionMode::Auto,
            &request,
        );

        assert!(matches!(
            accept_edit.decision,
            super::PermissionDecision::Deny
        ));
        assert!(matches!(auto.decision, super::PermissionDecision::Allow));
    }

    #[test]
    fn denial_tracker_promotes_to_ask_after_three_consecutive_denials() {
        let config = super::PermissionConfig::default();
        let request = super::PermissionRequest::from_tool_call(
            "execute_command",
            &serde_json::json!({ "command": "git reset --hard" }),
        );
        let mut tracker = super::DenialTracker::default();

        for _ in 0..3 {
            let outcome = super::evaluate_permission(
                &config,
                tracker.clone(),
                PermissionMode::Default,
                &request,
            );
            tracker = outcome.next_tracker;
        }

        let locked =
            super::evaluate_permission(&config, tracker, PermissionMode::Default, &request);

        assert!(matches!(locked.decision, super::PermissionDecision::Ask));
    }

    #[test]
    fn new_drops_legacy_permissions_json() {
        let root = unique_tmp_dir("legacy-perms");
        // Pre-existing legacy file at the data-dir root.
        std::fs::write(
            root.join("permissions.json"),
            r#"{"mode":"default","deny_list":[],"ask_list":[],"allow_list":[]}"#,
        )
        .unwrap();

        let _store = super::PermissionStore::new(root.clone());

        assert!(!root.join("permissions.json").exists(), "legacy permissions.json must be removed");
        // Per-user dir is created eagerly.
        assert!(root.join("permissions").is_dir());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn configs_and_trackers_are_isolated_per_user() {
        let root = unique_tmp_dir("per-user-perms");
        let store = super::PermissionStore::new(root.clone());

        // Alice configures a deny list; Bob leaves the default. Alice's
        // tool call must hit her deny list, Bob's must not see it.
        let mut alice_config = super::PermissionConfig::default();
        alice_config.deny_list.push("execute_command".to_string());
        store.set_config("user-alice", alice_config).unwrap();

        let alice_outcome = store
            .evaluate_tool_call(
                "user-alice",
                PermissionMode::Bypass,
                "execute_command",
                &serde_json::json!({ "command": "echo hi" }),
            )
            .unwrap();
        let bob_outcome = store
            .evaluate_tool_call(
                "user-bob",
                PermissionMode::Bypass,
                "execute_command",
                &serde_json::json!({ "command": "echo hi" }),
            )
            .unwrap();

        assert!(matches!(alice_outcome.decision, super::PermissionDecision::Deny));
        assert!(matches!(bob_outcome.decision, super::PermissionDecision::Allow));

        // The tracker counter must also be per-user: three denials
        // from Alice's side must not lock Bob out.
        for _ in 0..3 {
            let _ = store
                .evaluate_tool_call(
                    "user-alice",
                    PermissionMode::Default,
                    "execute_command",
                    &serde_json::json!({ "command": "git reset --hard" }),
                )
                .unwrap();
        }
        let bob_locked = store
            .evaluate_tool_call(
                "user-bob",
                PermissionMode::Default,
                "execute_command",
                &serde_json::json!({ "command": "git reset --hard" }),
            )
            .unwrap();
        // Bob hasn't accumulated any denials, so he should still get
        // the default Deny (not the Ask-from-circuit-breaker) decision.
        assert!(matches!(bob_locked.decision, super::PermissionDecision::Deny));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pending_prompts_are_scoped_per_user() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = super::PermissionStore::new(unique_tmp_dir("per-user-pending"));
            let request = super::PermissionPromptRequest {
                request_id: "req_1".to_string(),
                conversation_id: "conv".to_string(),
                tool_name: "execute_command".to_string(),
                arguments: serde_json::json!({ "command": "git status" }),
                reason: "Need approval".to_string(),
                mode: "auto".to_string(),
                content: Some("git status".to_string()),
                expires_at_ms: 0,
            };

            // Alice registers a pending prompt.
            let wait = store.wait_for_resolution(
                "user-alice",
                request.clone(),
                Duration::from_secs(1),
            );

            // Bob trying to resolve Alice's request must fail.
            let cross = store.resolve_request("user-bob", "req_1", true);
            assert!(cross.is_err(), "Bob must not be able to resolve Alice's prompt");

            // Alice resolves her own prompt successfully.
            store.resolve_request("user-alice", "req_1", true).unwrap();
            let approved = wait.await.unwrap();
            assert!(approved);
        });
    }
}
