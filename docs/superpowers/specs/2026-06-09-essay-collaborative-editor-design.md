# Essay Collaborative Editor — Design Spec

**Date**: 2026-06-09
**Status**: In Design
**Summary**: A new collaborative markdown essay editor with Obsidian-style Live Preview, heading folding, and cursor-level sync — built on CodeMirror 6 + Yjs, extending the existing CRDT sync infrastructure.

---

## 1. Motivation

The app currently has a Monaco-based code editor for `.py`, `.json`, and other code files. Markdown files (`.md`) open in Monaco as plain text. Users writing MCM/ICM competition papers need a dedicated writing experience:

- **Live Preview**: Write markdown naturally, see the rendered output inline (current line = source, others = rendered)
- **Section Folding**: Collapse sections by heading level for navigation in long papers
- **Cursor-level Sync**: Multiple team members editing simultaneously, each seeing others' cursors — like Overleaf

## 2. Non-Goals

- Comments / annotations / suggested edits
- Version history UI (CRDT snapshots already auto-persist)
- Rich media embedding beyond markdown images and LaTeX
- Mobile / responsive design (desktop-first, matching the app)

## 3. Architecture

```
app/projects/[id]/essay/page.tsx          ← Route page (new)
components/essay/
├─ essay-editor.tsx                         ← CodeMirror 6 core
├─ essay-topbar.tsx                         ← Title bar
├─ essay-sidebar.tsx                        ← Right file-tree panel
├─ essay-statusbar.tsx                      ← Bottom status bar
└─ use-essay-collab.ts                     ← Yjs connection hook
lib/codemirror/
├─ live-preview.ts                          ← Live Preview decoration plugin
├─ awareness.ts                             ← Yjs awareness → CM cursor adapter
└─ theme.ts                                 ← Dark CM theme
```

### 3.1 Dependency Stack

**New npm packages**:

| Package | Purpose |
|---------|---------|
| `@codemirror/view` | Editor view & decorations |
| `@codemirror/state` | Editor state management |
| `@codemirror/lang-markdown` | Markdown syntax + Live Preview hooks |
| `@codemirror/language` | Language support base |
| `@codemirror/fold` | Code folding (heading-based) |
| `@codemirror/commands` | Keyboard shortcuts |
| `@replit/codemirror-live-preview` | Live Preview plugin (battle-tested) |
| `y-codemirror.next` | Yjs binding for CodeMirror 6 |

**No new Rust dependencies** — the existing `yrs` crate handles server-side CRDT.

### 3.2 Route

```
/projects/[id]/essay?file=<file_id>
```

- `id`: project ID (from existing route)
- `file_id`: the file UUID (`files.id` in SQLite, or path for local Tauri files)

If `file_id` is missing, show an empty state prompting the user to open a `.md` file from the file tree.

### 3.3 Integration Point

In `modeler-workbench.tsx`, the existing `openFile()` function (line 1509) currently creates a code editor tab for all file types. Modify:

```typescript
// When language === "markdown", navigate instead of creating a tab
if (lang === "markdown" && file.id) {
  router.push(`/projects/${projectId}/essay?file=${file.id}`)
  return
}
```

---

## 4. Page Layout

```
┌──────────────────────────────────────────────────────────┐
│ ← Back   Essay Title (editable)     🟢 Synced   ●●○ 3   │  TopBar (h-10, 40px)
├───────────────────────────────────┬──────────────────────┤
│                                    │  📁 Project Files     │
│                                    │  ├─ data/            │
│    CodeMirror Editor               │  ├─ models/          │
│    (Live Preview Mode)             │  └─ figures/         │
│                                    │  ── drag handle ──   │
│    ## 2. Model                     │  (no bottom panel    │
│    The model assumes...            │   until research      │
│                                    │   system matures)    │
│                                    │                      │
├───────────────────────────────────┴──────────────────────┤
│ 1,234 words  |  § 3 of 8  |  💾 Saved · 2s ago           │  StatusBar (h-7, 28px)
└──────────────────────────────────────────────────────────┘
```

### 4.1 TopBar (`essay-topbar.tsx`)

- Back button (←) — navigates to `/projects/[id]`
- Inline-editable essay title (click to rename, updates both local file name and server)
- Sync indicator:
  - 🟢 Green dot + "Synced" — WebSocket connected, no pending changes
  - 🟡 Yellow + "Saving..." — pending local changes (debounced indicator)
  - 🔴 Red + "Offline" — WebSocket disconnected
- Collaboration avatars: colored circles with user initials, extracted from awareness state

### 4.2 Editor (`essay-editor.tsx`)

Core CodeMirror 6 instance with extensions:

```typescript
const extensions = [
  markdown({ extensions: [Strikethrough, TaskList, Table] }),
  foldGutter(),
  EditorView.lineWrapping,
  highlightSpecialChars(),
  history(),
  drawSelection(),
  bracketMatching(),
  closeBrackets(),
  highlightActiveLine(),
  // Yjs collaboration
  yCollab(ydoc.getText('content'), awareness, { undoManager }),
  // Awareness cursors
  awarenessCursor(awareness),
  // Live Preview
  livePreview(),
  // LaTeX widget (katex)
  latexWidget(),
  // Dark theme matching app
  essayTheme,
  // Editor config
  EditorView.updateListener.of(debouncedSave),
]
```

**Live Preview behavior** (via `@replit/codemirror-live-preview` or custom plugin):

1. Cursor is on line N → line N shows raw markdown (`#`, `**`, `$` visible)
2. All other lines hide markdown delimiters via decorations:
   - `# Title` → Title rendered large/bold, `# ` hidden
   - `**bold**` → rendered bold, `**` hidden
   - `` `code` `` → rendered with mono background, backticks hidden
   - `$E=mc^2$` → rendered via KaTeX, `$` delimiters hidden
3. Clicking on a rendered line moves cursor there, un-hides delimiters

**Folding**:

- `foldGutter` with markdown heading folding strategy
- Click fold icon on a `##` heading → collapses all content until next `##` or `#`
- Fold state is local (not synced via Yjs — each user folds independently)

### 4.3 Sidebar (`essay-sidebar.tsx`)

- Renders the project file tree using existing `FileNode` component
- Files are read-only view; clicking a non-`.md` file navigates back to the project's code editor
- Width: ~260px default, resizable via `react-resizable-panels`
- No research results panel yet (placeholder for future phase)

### 4.4 StatusBar (`essay-statusbar.tsx`)

- Word count (markdown-aware: exclude syntax characters)
- Current section indicator (e.g., "§ 3 of 8" — derived from heading positions)
- Save status text + timestamp ("Saved · 2s ago" or "Unsaved changes")

---

## 5. Collaboration Design

### 5.1 Yjs Document Structure

```
Y.Doc
  └─ content: Y.Text          ← the markdown source (synced)
  └─ awareness (Protocol)     ← cursor positions + user info
```

### 5.2 Awareness Protocol

Each client publishes:

```typescript
{
  user: {
    name: string,
    color: string,        // assigned client-side, consistent per user
    colorLight: string,
  },
  cursor: null | {        // null when editor loses focus
    anchor: Y.RelativePosition,
    head: Y.RelativePosition,
  },
}
```

- `y-codemirror.next`'s `awarenessCursor` extension renders other users' cursors as colored carets + name labels
- Selected text ranges show as colored highlights

### 5.3 Connection Lifecycle

```
EssayPage mounts
  → use-essay-collab(fileId) hook
    → create Y.Doc
    → create awareness
    → new YjsWebsocketProvider(doc, fileId)  // existing class
    → create CodeMirrorView with yCollab extension
    → setup awareness ↔ WS bridge

EssayPage unmounts
  → provider.destroy()
  → doc.destroy()
```

### 5.4 Backend Changes

**`server/src/sync/room.rs`** — Add awareness channel to SyncRoom:

```rust
pub awareness_tx: broadcast::Sender<Vec<u8>>,
```

**`server/src/sync/handlers.rs`** — Handle Awareness messages in WebSocket loop:

```
tokio::select! {
    // ... existing update branch
    // New: awareness branch
    Ok(awareness_update) = awareness_rx.recv() => {
        let msg = SyncMessage::Awareness { state: awareness_update };
        socket.send(Message::Text(serde_json::to_string(&msg).into())).await;
    }
}
```

New client handshake: send current awareness state of all online clients alongside SyncFull.

### 5.5 Frontend Provider Changes

Extend `lib/yjs-provider.ts`:

- On `awareness.on('update')`: serialize and send as `SyncMessage::Awareness`
- On WS receive with `type: "awareness"`: apply update to local awareness
- Add method `setAwareness(awareness: Awareness)` to provider

---

## 6. Markdown Extension Support

| Feature | Implementation |
|---------|---------------|
| Headings (# → ######) | `@codemirror/lang-markdown` built-in |
| Bold / Italic | `@codemirror/lang-markdown` built-in |
| Code spans / blocks | `@codemirror/lang-markdown` built-in |
| Tables | `@codemirror/lang-markdown` GFM extension |
| Task lists | `@codemirror/lang-markdown` GFM extension |
| LaTeX math ($...$, $$...$$) | Custom KaTeX widget (or `codemirror-latex`) |
| Images ![]() | `@codemirror/lang-markdown` built-in |
| Links []() | `@codemirror/lang-markdown` built-in |
| Block quotes | `@codemirror/lang-markdown` built-in |

---

## 7. Persistence

- **CRDT state**: Saved by existing `persist_state()` in `room.rs` when last client disconnects
- **File metadata**: Updated via `files.size` and `files.updated_at` in SQLite
- **Snapshots**: Existing `auto_snapshot_after_persist()` creates periodic snapshots (every disconnect when content changed)
- **Tauri local mode**: For local-only projects (no server), Yjs CRDT still works over Tauri's IPC; file content persisted via `write_file`

---

## 8. File Tree to Essay Navigation

### 8.1 Entry Points

1. **File tree `.md` click** in project page → `router.push(/projects/[id]/essay?file=<file_id>)`
2. **Direct URL** — shared link, opens essay editor for given file

### 8.2 Back Navigation

- TopBar ← button → `router.push(/projects/[id])` (back to project with code editor tabs)
- Keyboard shortcut: `Ctrl+Shift+E` (mnemonic: "Exit essay")

---

## 9. Error Handling

| Scenario | Behavior |
|----------|----------|
| File not found | Essay page shows 404 state with "File not found" + back link |
| Auth token missing | YjsWebsocketProvider logs warning, falls back to local-only editing |
| WebSocket disconnect | Editor stays editable; indicator turns red; auto-reconnect per existing provider logic |
| Sync conflict | Yjs CRDT auto-resolves (no manual merge needed) |
| Permission denied (read-only) | Editor opens in read-only mode; status bar shows "Viewing" |

---

## 10. Test Plan

### 10.1 Unit Tests (Rust)

- `SyncRoom` awareness broadcast: subscribed receivers get awareness updates
- `SyncRoom` awareness fan-out: multiple clients each get others' updates

### 10.2 Integration Tests (TypeScript)

- `live-preview.ts` decorations: markdown delimiters hidden except on active line
- `awareness.ts` cursor mapping: Yjs relative position ↔ CodeMirror absolute position

### 10.3 Manual Testing

- Two browser tabs open same essay → edit in one, see changes in other
- Cursor positions visible across tabs (different colors)
- Fold heading → section collapses; other user not affected
- Live Preview: move cursor → current line shows source, other lines show rendered
- Click rendered line → cursor moves, line becomes editable
- Disconnect network → indicator turns red; reconnect → sync resumes

---

## 11. Open Questions

1. **LaTeX**: Use `@replit/codemirror-live-preview`'s built-in LaTeX widget, or write a custom KaTeX one? Decision deferred to implementation — start with custom KaTeX widget since the app already bundles `katex`.
2. **On-disk file format**: The current sync system stores CRDT binary state in `crdt_docs`. For `.md` files created locally in Tauri mode, we also need to write clean markdown to disk. This means a dual-write: Yjs CRDT for sync, and plain `.md` write for local filesystem. Implementation will need a markdown serializer from the Yjs doc.

---

## 12. Implementation Phases

| Phase | Scope | Approx. Effort |
|-------|-------|----------------|
| 1 | CodeMirror setup + Live Preview + dark theme | Core editor |
| 2 | Essay page layout (TopBar / Sidebar / StatusBar) | UI shell |
| 3 | Yjs collaboration (content sync via existing WS) | Collaboration |
| 4 | Awareness cursors (backend + frontend) | Cursor sync |
| 5 | File tree → essay navigation integration | Integration |
| 6 | Folding, LaTeX, polish | Finishing |
