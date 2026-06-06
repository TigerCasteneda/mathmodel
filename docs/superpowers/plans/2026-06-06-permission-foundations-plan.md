# Permission Foundations Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the backend permission system foundations: string rule parsing and wildcard matching, persistent permission config, denial tracking, and runtime enforcement. `Auto` remains a first-class mode above `Accept Edit`.

**Architecture:** Introduce a dedicated `permissions.rs` module that owns permission config, rule matching, and denial tracking. Wire a managed `PermissionStore` into `runtime.rs` so tool execution decisions are made centrally before registry execution. Interactive ask prompts are explicitly deferred; `Ask` decisions return a clear backend error for now.

**Tech Stack:** Rust, Tauri v2, existing `PermissionMode`, JSON persistence in app data dir, `cargo test`, `cargo check`, `npx.cmd tsc --noEmit`.

---

### Task 1: Tests for Rules, Modes, and Denial Tracking

**Files:**
- Add: `src-tauri/src/ai/permissions.rs`
- Modify: `src-tauri/src/ai/mod.rs`

- [ ] **Step 1: Write failing tests**

Add tests for:

```rust
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
        super::runtime::PermissionMode::AcceptEdit,
        &request,
    );
    let auto = super::evaluate_permission(
        &config,
        super::DenialTracker::default(),
        super::runtime::PermissionMode::Auto,
        &request,
    );

    assert!(matches!(accept_edit.decision, super::PermissionDecision::Deny));
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
            super::runtime::PermissionMode::Default,
            &request,
        );
        tracker = outcome.next_tracker;
    }

    let locked = super::evaluate_permission(
        &config,
        tracker,
        super::runtime::PermissionMode::Default,
        &request,
    );

    assert!(matches!(locked.decision, super::PermissionDecision::Ask));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri`:

```powershell
cargo test parses_rule_with_tool_alias_and_wildcard_content
cargo test auto_mode_allows_low_risk_command_but_accept_edit_does_not
cargo test denial_tracker_promotes_to_ask_after_three_consecutive_denials
```

Expected: FAIL because `permissions.rs` does not exist yet.

### Task 2: Implement Permission Engine and Persistence

**Files:**
- Add: `src-tauri/src/ai/permissions.rs`
- Modify: `src-tauri/src/ai/runtime.rs`
- Modify: `src-tauri/src/ai/chat.rs`
- Modify: `src-tauri/src/ai/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add rule/config/store types**

Implement:
- `PermissionRule`
- `PermissionConfig { mode, deny_list, ask_list, allow_list }`
- `DenialTracker`
- `PermissionDecision`
- `PermissionRequest`
- `PermissionStore` backed by `permissions.json`

- [ ] **Step 2: Implement evaluation pipeline**

Order:
1. deny list
2. ask list
3. tool category check
4. mode default
5. allow list override

Mode semantics:
- `Default`: read/search only
- `AcceptEdit`: file edit/save allowed, no shell commands
- `Auto`: everything in `AcceptEdit`, plus low-risk shell commands
- `Bypass`: broad allow

- [ ] **Step 3: Wire runtime enforcement**

Before tool registry execution:
- construct a `PermissionRequest`
- evaluate against the store
- return structured error JSON for `Deny` / `Ask`
- update denial tracking state

Keep deferred-tool gating intact.

- [ ] **Step 4: Add config commands and managed state**

Add:
- `get_permission_config`
- `set_permission_config`

Manage `PermissionStore` in `lib.rs`, and use stored default mode when chat does not pass an explicit mode.

- [ ] **Step 5: Re-run focused permission tests**

Expected: PASS.

### Task 3: Verification and Commit

**Files:**
- No additional files unless cleanup is needed

- [ ] **Step 1: Run focused tests**

```powershell
cargo test parses_rule_with_tool_alias_and_wildcard_content
cargo test auto_mode_allows_low_risk_command_but_accept_edit_does_not
cargo test denial_tracker_promotes_to_ask_after_three_consecutive_denials
```

- [ ] **Step 2: Run broader verification**

```powershell
cargo test
cargo check
npx.cmd tsc --noEmit
```

- [ ] **Step 3: Inspect diff and commit**

Keep the commit scoped to permission foundations only.
