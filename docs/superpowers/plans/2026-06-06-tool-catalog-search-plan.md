# Tool Catalog Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade Modeler AI's native tool catalog and `tool_search` behavior to match the first slice of the 2026-06-06 Claude Code integration spec.

**Architecture:** Keep the existing Tauri/Rust runtime shape and add metadata plus pure search helpers inside `src-tauri/src/ai/runtime.rs`. The registered `tool_search` executor will call those helpers, while future executor and permission phases can reuse the new read-only/concurrency metadata.

**Tech Stack:** Rust, Tauri v2, `claude_code_rs` tool registry, existing `cargo check` and unit tests.

---

### Task 1: Remove Dead Legacy Tool Module

**Files:**
- Delete: `src-tauri/src/ai/tools.rs`
- Verify: `src-tauri/src/ai/mod.rs`

- [ ] **Step 1: Confirm no module reference exists**

Run:

```powershell
rg -n "mod tools|pub mod tools|tools::" src-tauri/src
```

Expected: no output.

- [ ] **Step 2: Delete the dead file**

Remove `src-tauri/src/ai/tools.rs`.

- [ ] **Step 3: Verify the delete is isolated**

Run:

```powershell
git status --short
```

Expected: deleted `src-tauri/src/ai/tools.rs` plus planned changes only.

### Task 2: Add Tool Metadata

**Files:**
- Modify: `src-tauri/src/ai/runtime.rs`

- [ ] **Step 1: Write metadata tests**

Add tests under `#[cfg(test)] mod tests` in `runtime.rs`:

```rust
#[test]
fn catalog_exposes_read_only_and_concurrency_metadata() {
    let read = tool_by_name_or_alias("file_read").expect("file_read exists");
    assert!(read.is_read_only);
    assert!(read.is_concurrency_safe);

    let write = tool_by_name_or_alias("file_write").expect("file_write exists");
    assert!(!write.is_read_only);
    assert!(!write.is_concurrency_safe);

    let bash = tool_by_name_or_alias("Bash").expect("Bash alias resolves");
    assert_eq!(bash.name, "execute_command");
    assert!(!bash.is_read_only);
    assert!(!bash.is_concurrency_safe);

    let search = tool_by_name_or_alias("tool_search").expect("tool_search exists");
    assert!(search.is_read_only);
    assert!(search.is_concurrency_safe);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test catalog_exposes_read_only_and_concurrency_metadata
```

from `src-tauri`.

Expected: FAIL because `tool_by_name_or_alias` and metadata fields do not exist yet.

- [ ] **Step 3: Add metadata fields and catalog values**

Update `ToolCatalogEntry`:

```rust
struct ToolCatalogEntry {
    name: &'static str,
    description: &'static str,
    exposure: ToolExposure,
    keywords: &'static [&'static str],
    search_hint: &'static str,
    aliases: &'static [&'static str],
    is_concurrency_safe: bool,
    is_read_only: bool,
}
```

Add a `tool_search` catalog entry and populate every entry conservatively:

```rust
search_hint: "Find deferred tools by name or capability.",
aliases: &[],
is_concurrency_safe: true,
is_read_only: true,
```

Use `aliases: &["Bash"]` for `execute_command`, `aliases: &["Read"]` for `file_read`, `aliases: &["Write"]` for `file_write`, and `aliases: &["Edit"]` for `file_edit`.

- [ ] **Step 4: Add alias lookup helper**

Add:

```rust
fn tool_by_name_or_alias(name: &str) -> Option<&'static ToolCatalogEntry> {
    TOOL_CATALOG.iter().find(|entry| {
        entry.name.eq_ignore_ascii_case(name)
            || entry.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(name))
    })
}
```

Update existing `tool_by_name()` callers that accept user/model names to use `tool_by_name_or_alias()`.

- [ ] **Step 5: Run metadata test**

Run:

```powershell
cargo test catalog_exposes_read_only_and_concurrency_metadata
```

Expected: PASS.

### Task 3: Upgrade Tool Search Matching

**Files:**
- Modify: `src-tauri/src/ai/runtime.rs`

- [ ] **Step 1: Write search behavior tests**

Add tests:

```rust
#[test]
fn tool_search_supports_select_prefix_and_aliases() {
    let matches = search_tool_catalog("select:Bash,search_files", &[], 8);
    let names = matches.iter().map(|entry| entry.name).collect::<Vec<_>>();
    assert_eq!(names, vec!["execute_command", "search_files"]);
}

#[test]
fn tool_search_requires_plus_terms() {
    let matches = search_tool_catalog("+shell pattern", &[], 8);
    let names = matches.iter().map(|entry| entry.name).collect::<Vec<_>>();
    assert!(names.contains(&"execute_command"));
    assert!(!names.contains(&"search_files"));
}

#[test]
fn tool_search_scores_camel_case_alias_hint_and_description() {
    let bash_matches = search_tool_catalog("bash", &[], 3);
    assert_eq!(bash_matches.first().map(|entry| entry.name), Some("execute_command"));

    let docs_matches = search_tool_catalog("markdown webpage", &[], 3);
    assert_eq!(docs_matches.first().map(|entry| entry.name), Some("fetch_url"));

    let background_matches = search_tool_catalog("background subagent", &[], 3);
    assert_eq!(
        background_matches.first().map(|entry| entry.name),
        Some("start_background_task")
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test tool_search_
```

from `src-tauri`.

Expected: FAIL because `search_tool_catalog()` does not exist and current executor does not support these query forms.

- [ ] **Step 3: Implement query parsing and scoring helpers**

Add pure helpers:

```rust
fn search_tool_catalog(
    query: &str,
    selected: &[String],
    limit: usize,
) -> Vec<&'static ToolCatalogEntry>
```

Rules:
- Search only `ToolExposure::Deferred` for keyword queries.
- `select:A,B,C` and the existing `select` array use direct selection by canonical name or alias.
- `+term` terms must match name parts, alias, search hint, keyword, or description.
- Score exact name part: 10, partial name part: 5, alias: 5, search hint: 4, keyword: 2, description: 2.
- Deduplicate by canonical tool name.
- Sort by descending score, then shorter name, then lexical name.

- [ ] **Step 4: Wire executor to helper**

In `ToolSearchExecutor::execute`, replace the inline scoring with:

```rust
let matches = search_tool_catalog(&query, &selected, limit);
```

Return `search_hint`, `aliases`, `is_concurrency_safe`, and `is_read_only` in each returned tool object.

- [ ] **Step 5: Run search tests**

Run:

```powershell
cargo test tool_search_
```

Expected: PASS.

### Task 4: Public Metadata Helpers for Later Phases

**Files:**
- Modify: `src-tauri/src/ai/runtime.rs`

- [ ] **Step 1: Write helper tests**

Add:

```rust
#[test]
fn tool_metadata_helpers_use_aliases_and_fail_closed() {
    assert!(is_tool_read_only("Read"));
    assert!(is_tool_concurrency_safe("tool_search"));
    assert!(!is_tool_read_only("Write"));
    assert!(!is_tool_concurrency_safe("definitely_missing_tool"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test tool_metadata_helpers_use_aliases_and_fail_closed
```

Expected: FAIL because helper functions do not exist.

- [ ] **Step 3: Implement helpers**

Add:

```rust
pub fn is_tool_concurrency_safe(name: &str) -> bool {
    tool_by_name_or_alias(name)
        .map(|entry| entry.is_concurrency_safe)
        .unwrap_or(false)
}

pub fn is_tool_read_only(name: &str) -> bool {
    tool_by_name_or_alias(name)
        .map(|entry| entry.is_read_only)
        .unwrap_or(false)
}
```

- [ ] **Step 4: Run helper test**

Run:

```powershell
cargo test tool_metadata_helpers_use_aliases_and_fail_closed
```

Expected: PASS.

### Task 5: Verification and Commit

**Files:**
- Verify all touched files.

- [ ] **Step 1: Run Rust verification**

Run:

```powershell
cargo check
cargo test
```

from `src-tauri`.

Expected: both exit 0.

- [ ] **Step 2: Run frontend typecheck**

Run:

```powershell
npx.cmd tsc --noEmit
```

from repo root.

Expected: exit 0.

- [ ] **Step 3: Review diff**

Run:

```powershell
git diff -- src-tauri/src/ai/runtime.rs src-tauri/src/ai/tools.rs docs/superpowers/plans/2026-06-06-tool-catalog-search-plan.md
```

Expected: only Part 1 tool catalog/search changes.

- [ ] **Step 4: Commit**

Run:

```powershell
git add docs/superpowers/plans/2026-06-06-tool-catalog-search-plan.md src-tauri/src/ai/runtime.rs src-tauri/src/ai/tools.rs
git commit -m "Add Claude Code style tool search metadata"
```

Expected: commit created.
