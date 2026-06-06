use crate::ai::runtime::PermissionMode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;

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

#[derive(Clone)]
pub struct PermissionStore {
    path: PathBuf,
    state: Arc<Mutex<PermissionState>>,
}

#[derive(Debug, Clone)]
struct PermissionState {
    config: PermissionConfig,
    tracker: DenialTracker,
}

impl PermissionStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let path = app_data_dir.join("permissions.json");
        let config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<PermissionConfig>(&content).ok())
            .unwrap_or_default();

        Self {
            path,
            state: Arc::new(Mutex::new(PermissionState {
                config,
                tracker: DenialTracker::default(),
            })),
        }
    }

    pub fn get_config(&self) -> Result<PermissionConfig, String> {
        let state = self.state.lock().map_err(|error| error.to_string())?;
        Ok(state.config.clone())
    }

    pub fn set_config(&self, config: PermissionConfig) -> Result<PermissionConfig, String> {
        validate_mode_value(config.mode.as_deref())?;
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.config = config.clone();
        persist_permission_config(&self.path, &state.config)?;
        Ok(config)
    }

    pub fn configured_mode(&self) -> Result<Option<String>, String> {
        let state = self.state.lock().map_err(|error| error.to_string())?;
        Ok(state.config.mode.clone())
    }

    pub fn evaluate_tool_call(
        &self,
        mode: PermissionMode,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<PermissionOutcome, String> {
        let request = PermissionRequest::from_tool_call(tool_name, arguments);
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        let outcome = evaluate_permission(&state.config, state.tracker.clone(), mode, &request);
        state.tracker = outcome.next_tracker.clone();
        Ok(outcome)
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
    store: State<'_, PermissionStore>,
) -> Result<PermissionConfig, String> {
    store.get_config()
}

#[tauri::command]
pub fn set_permission_config(
    config: PermissionConfig,
    store: State<'_, PermissionStore>,
) -> Result<PermissionConfig, String> {
    store.set_config(config)
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
    use crate::ai::runtime::PermissionMode;

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
}
