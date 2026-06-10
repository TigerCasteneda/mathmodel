# Essay Inline AI Ghost Text — Design Spec

**Date**: 2026-06-10
**Status**: In Design
**Summary**: Tabby-style inline AI ghost text for the essay editor. AI suggests gray italic continuation text that floats ahead of the cursor. User presses Tab to accept, Esc or continues typing to dismiss. Context includes the current essay + all `.md` files from the project (RAG-lite). Adapts architecture patterns from Tabby's codeCompletion module.

## 1. Motivation

Users writing MCM/ICM papers need an inline AI assistant that feels like Copilot/Tabby — not a chat panel. The AI should silently suggest continuations based on context, without breaking writing flow.

## 2. Non-Goals

- Full AI chat panel in essay (already exists in chat tab)
- Multi-turn conversation
- Tool execution (file operations, web search) during inline completion
- Tabby's adaptive debouncing (fixed 1.5s is fine for prose)

## 3. Architecture

```
lib/codemirror/ghost-text.ts         ← Ghost Text ViewPlugin
components/essay/essay-ghost.ts      ← Context collector + AI caller
components/essay/essay-editor.tsx    ← Load ghost plugin + pass deps
```

### 3.1 Data Flow

```
User stops typing 1.5s
  → AbortController cancels any pending request
  → Collect context: prefix (500 chars) + suffix (200 chars) + project .md files
  → Build prompt → Tauri aiChat → SSE stream
  → GhostWidget renders tokens as gray italic @ opacity 0.3
  → User presses Tab → insert into Y.Text
  → User presses Esc or types → abort stream, remove widget
```

### 3.2 Ghost Text Widget (CodeMirror)

- `GhostWidget extends WidgetType` — a DOM element positioned after the cursor
- `Decoration.widget({ widget, side: 0 })` — side:0 places it at cursor
- CSS: `color: rgba(255,255,255,0.3); font-style: italic;`
- Updated on each SSE token via `view.dispatch()`
- Removed on abort/accept/dismiss

### 3.3 Keybindings

| Key | Action |
|-----|--------|
| Tab | Accept ghost text → insert into Y.Text at cursor |
| Esc | Dismiss ghost text |
| Any typing | Dismiss ghost text, start debounce timer |

## 4. Prompt Design (Tabby-inspired)

```
[System]
You are an academic writing assistant. Continue the essay naturally.
File: {essay_filename}

[Reference materials from the project]
{other .md files, each truncated to 2000 chars, max 5 files}

[Context]
## Before cursor:
{cursor_prefix last 500 chars}

## After cursor:
{cursor_suffix first 200 chars}

Continue writing. Match style, tone, and detail level. Do NOT repeat existing
content. Write in the same language as the text.
```

### 4.1 Context Collection

| Source | Content | Limit |
|--------|---------|-------|
| Current essay (before cursor) | `ytext.toString()` slice | Last 500 chars |
| Current essay (after cursor) | As above | First 200 chars |
| Other project `.md` files | `tauriApi.listFiles` → `.md` → `readFile` | First 2000 chars each, max 5 |

## 5. Concurrency & Cancellation (Tabby's Mutex Pattern)

- `AbortController` per request
- New request → `controller.abort()` on previous
- SSE stream observes `signal.aborted`
- CompletionCache: hash of (filename + cursor_position + prefix_suffix_hash) → skip if cached

## 6. Post-processing

- Strip leading whitespace that would create double newlines
- Trim to sentence boundary (`. ` or `.\n` or `。`)
- Drop if result is < 5 characters
- Drop if result duplicates existing text in cursor suffix

## 7. Files to Create/Modify

| File | Action | Purpose |
|------|--------|---------|
| `lib/codemirror/ghost-text.ts` | **NEW** | GhostWidget + GhostTextPlugin + accept/dismiss logic |
| `components/essay/essay-ghost.ts` | **NEW** | Context collector, prompt builder, AI caller, debounce |
| `components/essay/essay-editor.tsx` | Modify | Load ghost plugin, pass ydoc/ytext/ghostState |
| `lib/tauri-api.ts` | Modify | Export `aiChatWithStream` helper for non-chat use |

## 8. Verification

1. Open essay editor → type some text → stop for 1.5s → gray italic ghost text appears
2. Tab → ghost text inserted, cursor moves to end
3. Esc → ghost text dismissed
4. Type during streaming → ghost text dismissed, old request aborted
5. `npx tsc --noEmit` — zero errors
6. `cargo check` + `cargo test` — no regressions (53 tests)
