# Essay Collaborative Editor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an Obsidian-style collaborative markdown essay editor with Live Preview, heading folding, and cursor-level sync — CodeMirror 6 + Yjs on the existing CRDT WebSocket backend.

**Architecture:** New route `/projects/[id]/essay?file=<file_id>` hosts a full-page CodeMirror 6 editor with a right-side file tree sidebar. Collaboration reuses the existing `YjsWebsocketProvider` extended with awareness support. The backend `SyncRoom` gains an awareness broadcast channel. `.md` file clicks in the project file tree navigate here instead of opening a Monaco code tab.

**Tech Stack:** CodeMirror 6 (`@codemirror/view`, `@codemirror/state`, `@codemirror/lang-markdown`, `@codemirror/fold`, `@codemirror/commands`, `@codemirror/language`), `y-codemirror.next`, existing `yjs` / `yrs` / `react-resizable-panels` / `katex`

---

## Task 1: Install CodeMirror 6 Dependencies

**Files:**
- Modify: `package.json`

- [ ] **Step 1: Install CodeMirror packages**

```bash
cd D:/5_user/mathmodel && npm install \
  @codemirror/view \
  @codemirror/state \
  @codemirror/lang-markdown \
  @codemirror/language \
  @codemirror/fold \
  @codemirror/commands \
  @codemirror/search \
  y-codemirror.next
```

- [ ] **Step 2: Verify install**

```bash
cd D:/5_user/mathmodel && node -e "
const cm = require('@codemirror/view');
const md = require('@codemirror/lang-markdown');
const fold = require('@codemirror/fold');
console.log('@codemirror/view:', !!cm.EditorView);
console.log('@codemirror/lang-markdown:', !!md.markdown);
console.log('@codemirror/fold:', !!fold.foldGutter);
"
```

Expected: all print `true`

- [ ] **Step 3: Commit**

```bash
git add package.json package-lock.json
git commit -m "chore: add CodeMirror 6 and y-codemirror.next dependencies"
```

---

## Task 2: CodeMirror Dark Theme

**Files:**
- Create: `lib/codemirror/theme.ts`

- [ ] **Step 1: Create theme file**

```typescript
// lib/codemirror/theme.ts
import { EditorView } from "@codemirror/view"

export const essayTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "#0d0d0d",
      color: "#d4d4d4",
      fontSize: "15px",
      height: "100%",
    },
    ".cm-content": {
      caretColor: "#e0e0e0",
      fontFamily: "'Geist Mono', 'JetBrains Mono', 'Fira Code', monospace",
      padding: "16px 24px",
      lineHeight: "1.75",
      maxWidth: "800px",
      margin: "0 auto",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "#e0e0e0",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "#264f78",
    },
    ".cm-activeLine": {
      backgroundColor: "#ffffff06",
    },
    ".cm-gutters": {
      backgroundColor: "#0d0d0d",
      color: "#555",
      border: "none",
      paddingRight: "8px",
    },
    ".cm-foldGutter .cm-gutterElement": {
      color: "#555",
      cursor: "pointer",
      padding: "0 4px",
    },
    ".cm-foldGutter .cm-gutterElement:hover": {
      color: "#888",
    },
    ".cm-foldPlaceholder": {
      backgroundColor: "#1a1a1a",
      border: "1px solid #333",
      color: "#888",
      borderRadius: "3px",
      padding: "0 6px",
      margin: "0 2px",
    },
    // Heading styles for Live Preview rendering
    ".cm-heading": {
      fontWeight: "600",
    },
    ".cm-heading1": {
      fontSize: "1.6em",
      fontWeight: "700",
      marginTop: "0.6em",
    },
    ".cm-heading2": {
      fontSize: "1.35em",
      fontWeight: "600",
      marginTop: "0.5em",
    },
    ".cm-heading3": {
      fontSize: "1.15em",
      fontWeight: "600",
    },
    ".cm-strong": {
      fontWeight: "700",
    },
    ".cm-emphasis": {
      fontStyle: "italic",
    },
    ".cm-strikethrough": {
      textDecoration: "line-through",
    },
    ".cm-code": {
      fontFamily: "'Geist Mono', 'JetBrains Mono', monospace",
      fontSize: "0.9em",
    },
    ".cm-link": {
      color: "#569cd6",
      textDecoration: "underline",
    },
    ".cm-url": {
      color: "#4a90d9",
    },
    // Hide markdown delimiters when not on active line (Live Preview)
    ".cm-line:not(.cm-activeLine) .cm-formatting": {
      opacity: "0",
    },
    ".cm-line:not(.cm-activeLine) .cm-formatting-header": {
      display: "none",
    },
    ".cm-line:not(.cm-activeLine) .cm-formatting-quote": {
      display: "none",
    },
    ".cm-line:not(.cm-activeLine) .cm-formatting-list": {
      opacity: "0",
    },
    // Always show task list checkboxes
    ".cm-task-marker": {
      opacity: "1 !important",
    },
    // Table styling
    ".cm-table-widget": {
      overflowX: "auto",
      padding: "4px 0",
    },
    ".cm-table-widget table": {
      borderCollapse: "collapse",
      width: "100%",
    },
    ".cm-table-widget td, .cm-table-widget th": {
      border: "1px solid #333",
      padding: "4px 8px",
      textAlign: "left",
    },
  },
  { dark: true },
)

export const essayHighlightStyle = EditorView.baseTheme({
  // Use baseTheme for highlight style overrides if needed
})
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit lib/codemirror/theme.ts
```

Expected: no errors (or known project-wide errors only)

- [ ] **Step 3: Commit**

```bash
git add lib/codemirror/theme.ts
git commit -m "feat: add CodeMirror 6 dark theme for essay editor"
```

---

## Task 3: Live Preview Decoration Plugin

**Files:**
- Create: `lib/codemirror/live-preview.ts`

- [ ] **Step 1: Create live preview plugin**

```typescript
// lib/codemirror/live-preview.ts
import {
  ViewPlugin,
  Decoration,
  DecorationSet,
  WidgetType,
  type EditorView,
  type PluginValue,
  type ViewUpdate,
} from "@codemirror/view"
import { syntaxTree } from "@codemirror/language"
import { RangeSetBuilder } from "@codemirror/state"

// ─── Inline LaTeX Widget ──────────────────────────────

class InlineMathWidget extends WidgetType {
  constructor(readonly latex: string) {
    super()
  }

  eq(other: InlineMathWidget) {
    return other.latex === this.latex
  }

  toDOM() {
    const span = document.createElement("span")
    span.className = "cm-latex-inline"
    span.setAttribute("data-latex", this.latex)
    // Render placeholder; katex rendering happens in a follow-up if needed
    span.textContent = this.latex
    span.style.color = "#d4a574"
    span.style.fontStyle = "italic"
    return span
  }

  ignoreEvent() {
    return false
  }
}

// ─── Markdown Formatting Node Types ───────────────────

const FORMATTING_DELIMITER = new Set([
  "HeaderMark",
  "EmphasisMark",
  "StrongMark",
  "StrikethroughMark",
  "CodeMark",
  "QuoteMark",
  "ListMark",
  "LinkMark",
  "ImageMark",
  "URL",
])

// ─── Plugin ──────────────────────────────────────────

class LivePreviewPlugin implements PluginValue {
  decorations: DecorationSet

  constructor(view: EditorView) {
    this.decorations = this.buildDecorations(view)
  }

  update(update: ViewUpdate) {
    if (
      update.docChanged ||
      update.selectionSet ||
      update.viewportChanged
    ) {
      this.decorations = this.buildDecorations(update.view)
    }
  }

  buildDecorations(view: EditorView): DecorationSet {
    const builder = new RangeSetBuilder<Decoration>()
    const { state } = view
    const doc = state.doc

    // Find the active line (where cursor is)
    const cursorLine = state.selection.main.head
      ? doc.lineAt(state.selection.main.head).number
      : -1

    // Walk syntax tree for markdown nodes
    syntaxTree(state).iterate({
      enter(node) {
        const nodeName = node.name
        const from = node.from
        const to = node.to

        // Skip if node spans the active line — let user see raw syntax there
        const nodeStartLine = doc.lineAt(from).number
        const nodeEndLine = doc.lineAt(Math.min(to, doc.length)).number
        if (
          cursorLine >= nodeStartLine &&
          cursorLine <= nodeEndLine
        ) {
          return
        }

        // Hide markdown formatting delimiters
        if (FORMATTING_DELIMITER.has(nodeName)) {
          builder.add(
            from,
            to,
            Decoration.replace({
              inclusive: true,
            }),
          )
          return
        }

        // Render inline math with KaTeX widget
        if (nodeName === "InlineMath" || nodeName === "Math") {
          const mathText = doc.sliceString(from + 1, to - 1)
          builder.add(
            from,
            to,
            Decoration.replace({
              widget: new InlineMathWidget(mathText),
              inclusive: false,
            }),
          )
          return
        }

        // Styling for rendered headings (header content, not the marks)
        if (nodeName.startsWith("ATXHeading")) {
          // The header text gets a class; the marks are already hidden above
          // ...content node handled via content styling
        }
      },
    })

    return builder.finish()
  }

  destroy() {}
}

// ─── Export ───────────────────────────────────────────

export function livePreview() {
  return ViewPlugin.fromClass(LivePreviewPlugin, {
    decorations: (plugin) => plugin.decorations,
  })
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit lib/codemirror/live-preview.ts
```

- [ ] **Step 3: Commit**

```bash
git add lib/codemirror/live-preview.ts
git commit -m "feat: add Live Preview decoration plugin for essay editor"
```

---

## Task 4: Awareness to CodeMirror Cursor Adapter

**Files:**
- Create: `lib/codemirror/awareness.ts`

- [ ] **Step 1: Create awareness adapter**

```typescript
// lib/codemirror/awareness.ts
import * as Y from "yjs"
import type { Awareness } from "y-protocols/awareness"

// Color palette for remote cursors
const CURSOR_COLORS = [
  "#f87171", // red
  "#60a5fa", // blue
  "#34d399", // green
  "#fbbf24", // yellow
  "#a78bfa", // purple
  "#f472b6", // pink
  "#38bdf8", // sky
  "#fb923c", // orange
]

// Persist user color per session so same user gets same color on reconnect
let userColorIndex = 0
const userColorMap = new Map<number, string>()

function assignColor(clientId: number): string {
  if (!userColorMap.has(clientId)) {
    userColorMap.set(
      clientId,
      CURSOR_COLORS[userColorIndex % CURSOR_COLORS.length],
    )
    userColorIndex++
  }
  return userColorMap.get(clientId)!
}

// ─── Awareness state helpers ─────────────────────────

export interface AwarenessUserInfo {
  name: string
  color: string
  colorLight: string
}

/**
 * Set local user info in awareness.
 * Call this once after creating the awareness instance.
 */
export function setLocalUserInfo(
  awareness: Awareness,
  user: AwarenessUserInfo,
) {
  awareness.setLocalStateField("user", user)
}

/**
 * Extract user info from an awareness state entry.
 */
export function getUserInfo(
  state: Record<string, unknown> | null,
): AwarenessUserInfo | null {
  if (!state) return null
  const user = state.user as AwarenessUserInfo | undefined
  return user ?? null
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit lib/codemirror/awareness.ts
```

- [ ] **Step 3: Commit**

```bash
git add lib/codemirror/awareness.ts
git commit -m "feat: add Yjs awareness to CodeMirror cursor adapter"
```

---

## Task 5: Extend YjsWebsocketProvider with Awareness

**Files:**
- Modify: `lib/yjs-provider.ts`

- [ ] **Step 1: Extend YjsWebsocketProvider to support awareness**

Replace `lib/yjs-provider.ts` with the extended version. The key changes are:

1. Accept an optional `Awareness` instance in constructor
2. Listen for local awareness changes and send them via WS
3. Handle incoming `SyncMessage::Awareness` and apply to local awareness

```typescript
// lib/yjs-provider.ts
import * as Y from "yjs"
import { getToken, getWebSocketBase } from "@/lib/api"

export interface SyncMessage {
  type: "sync_update" | "sync_full" | "awareness"
  update?: number[]
  state?: number[]
}

// Awareness protocol types (subset of y-protocols/awareness)
export interface AwarenessProtocol {
  on(
    event: "update",
    handler: (
      changes: {
        added: number[]
        updated: number[]
        removed: number[]
      },
      origin: unknown,
    ) => void,
  ): void
  off(
    event: "update",
    handler: (
      changes: {
        added: number[]
        updated: number[]
        removed: number[]
      },
      origin: unknown,
    ) => void,
  ): void
  getStates(): Map<number, Record<string, unknown>>
  getLocalState(): Record<string, unknown> | null
  setLocalState(state: Record<string, unknown> | null): void
  setLocalStateField(field: string, value: unknown): void
  /** Encode awareness update for a list of changed client IDs */
  encodeAwarenessUpdate(
    clients: number[],
    states: Map<number, Record<string, unknown>>,
  ): Uint8Array
  /** Apply an incoming awareness update */
  applyAwarenessUpdate(
    update: Uint8Array,
    origin: unknown,
  ): void
}

export class YjsWebsocketProvider {
  private ws: WebSocket | null = null
  private doc: Y.Doc
  private fileId: string
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private destroyed = false
  private updateHandler: (update: Uint8Array, origin: unknown) => void
  private awareness: AwarenessProtocol | null = null
  private awarenessUpdateHandler:
    | ((changes: {
        added: number[]
        updated: number[]
        removed: number[]
      }, origin: unknown) => void)
    | null = null
  private _synced = false

  constructor(
    doc: Y.Doc,
    fileId: string,
    awareness?: AwarenessProtocol,
  ) {
    this.doc = doc
    this.fileId = fileId
    this.awareness = awareness ?? null

    this.updateHandler = (update: Uint8Array, origin: unknown) => {
      if (origin === this) return
      if (this.ws?.readyState === WebSocket.OPEN) {
        const msg: SyncMessage = {
          type: "sync_update",
          update: Array.from(update),
        }
        this.ws.send(JSON.stringify(msg))
      }
    }

    // Bind awareness if provided
    if (this.awareness) {
      this.awarenessUpdateHandler = (
        changes: { added: number[]; updated: number[]; removed: number[] },
        origin: unknown,
      ) => {
        if (origin === this) return
        if (
          changes.added.length === 0 &&
          changes.updated.length === 0 &&
          changes.removed.length === 0
        )
          return
        // Collect changed client IDs and encode their states
        const changedClients = [
          ...changes.added,
          ...changes.updated,
          ...changes.removed,
        ]
        if (changedClients.length === 0) return
        if (this.ws?.readyState === WebSocket.OPEN) {
          const states = this.awareness!.getStates()
          const encoded = this.awareness!.encodeAwarenessUpdate(
            changedClients,
            states,
          )
          const msg: SyncMessage = {
            type: "awareness",
            state: Array.from(encoded),
          }
          this.ws.send(JSON.stringify(msg))
        }
      }
      this.awareness.on("update", this.awarenessUpdateHandler)
    }

    this.connect()
    this.doc.on("update", this.updateHandler)
  }

  get synced() {
    return this._synced
  }

  private async connect() {
    if (this.destroyed) return
    const token = getToken()
    if (!token) {
      console.warn("[YjsWS] missing auth token")
      return
    }

    const base = await getWebSocketBase()
    if (this.destroyed) return
    const url = `${base}/sync?file_id=${encodeURIComponent(this.fileId)}&token=${encodeURIComponent(token)}`
    this.ws = new WebSocket(url)

    this.ws.onopen = () => {
      console.log("[YjsWS] connected", this.fileId)
    }

    this.ws.onmessage = (event) => {
      try {
        const msg: SyncMessage = JSON.parse(event.data)

        // Handle content sync
        if (
          msg.type === "sync_full" &&
          msg.state
        ) {
          const state = new Uint8Array(msg.state)
          Y.applyUpdate(this.doc, state, this)
          this._synced = true
          return
        }

        if (
          msg.type === "sync_update" &&
          msg.update
        ) {
          const update = new Uint8Array(msg.update)
          Y.applyUpdate(this.doc, update, this)
          return
        }

        // Handle awareness
        if (
          msg.type === "awareness" &&
          msg.state &&
          this.awareness
        ) {
          const update = new Uint8Array(msg.state)
          this.awareness.applyAwarenessUpdate(update, this)
        }
      } catch (e) {
        console.error("[YjsWS] parse error", e)
      }
    }

    this.ws.onclose = () => {
      this._synced = false
      if (!this.destroyed) {
        console.log("[YjsWS] disconnected, reconnecting in 2s")
        this.reconnectTimer = setTimeout(() => this.connect(), 2000)
      }
    }

    this.ws.onerror = (err) => {
      console.error("[YjsWS] error", err)
    }
  }

  destroy() {
    this.destroyed = true
    this._synced = false
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.doc.off("update", this.updateHandler)
    if (this.awareness && this.awarenessUpdateHandler) {
      this.awareness.off("update", this.awarenessUpdateHandler)
    }
    this.ws?.close()
  }
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit lib/yjs-provider.ts
```

- [ ] **Step 3: Commit**

```bash
git add lib/yjs-provider.ts
git commit -m "feat: extend YjsWebsocketProvider with awareness support"
```

---

## Task 6: Yjs Collaboration Hook

**Files:**
- Create: `components/essay/use-essay-collab.ts`

- [ ] **Step 1: Create the collaboration hook**

```typescript
// components/essay/use-essay-collab.ts
"use client"

import { useEffect, useRef } from "react"
import * as Y from "yjs"
import { YjsWebsocketProvider } from "@/lib/yjs-provider"
import type { AwarenessProtocol } from "@/lib/yjs-provider"

interface UseEssayCollabOptions {
  fileId: string
  initialContent?: string
  readOnly?: boolean
  onSynced?: () => void
}

interface UseEssayCollabResult {
  ydoc: Y.Doc
  ytext: Y.Text
  awareness: AwarenessProtocol | null
  provider: YjsWebsocketProvider | null
  synced: boolean
}

/**
 * Hook that manages the Yjs document lifecycle for an essay file.
 * Creates a Y.Doc, connects via WebSocket, and returns the shared types.
 */
export function useEssayCollab({
  fileId,
  initialContent,
  readOnly,
  onSynced,
}: UseEssayCollabOptions): UseEssayCollabResult {
  const ydocRef = useRef<Y.Doc | null>(null)
  const providerRef = useRef<YjsWebsocketProvider | null>(null)
  const awarenessRef = useRef<AwarenessProtocol | null>(null)

  // Create once per fileId
  if (!ydocRef.current || ydocRef.current.guid !== fileId) {
    // Destroy previous
    providerRef.current?.destroy()
    ydocRef.current?.destroy()

    const doc = new Y.Doc()
    // Seed with initial content if Y.Text is empty
    const ytext = doc.getText("content")
    if (initialContent && ytext.toString() === "") {
      ytext.insert(0, initialContent)
    }

    ydocRef.current = doc
  }

  const ydoc = ydocRef.current
  const ytext = ydoc.getText("content")

  useEffect(() => {
    // Load awareness lazily (client-side only, needs y-protocols/awareness)
    let awareness: AwarenessProtocol | null = null
    let provider: YjsWebsocketProvider | null = null
    let cancelled = false

    void import("y-protocols/awareness").then((mod) => {
      if (cancelled) return
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      awareness = new (mod as any).Awareness(ydoc) as AwarenessProtocol
      awarenessRef.current = awareness
      provider = new YjsWebsocketProvider(ydoc, fileId, awareness)
      providerRef.current = provider

      if (readOnly) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ;(awareness as any).setLocalState(null)
      }

      // Signal synced once we get the first full state
      const checkSynced = setInterval(() => {
        if (provider?.synced) {
          clearInterval(checkSynced)
          onSynced?.()
        }
      }, 100)
    })

    return () => {
      cancelled = true
      provider?.destroy()
      // Don't destroy ydoc here — it's owned by the ref and reused
    }
  }, [fileId, readOnly, onSynced, ydoc])

  return {
    ydoc,
    ytext,
    awareness: awarenessRef.current,
    provider: providerRef.current,
    synced: providerRef.current?.synced ?? false,
  }
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit components/essay/use-essay-collab.ts
```

- [ ] **Step 3: Commit**

```bash
git add components/essay/use-essay-collab.ts
git commit -m "feat: add Yjs collaboration hook for essay editor"
```

---

## Task 7: EssayEditor — CodeMirror 6 Core Component

**Files:**
- Create: `components/essay/essay-editor.tsx`

- [ ] **Step 1: Create the CodeMirror 6 editor component**

```typescript
// components/essay/essay-editor.tsx
"use client"

import { useEffect, useRef } from "react"
import { EditorView, keymap, placeholder } from "@codemirror/view"
import { EditorState } from "@codemirror/state"
import { markdown, markdownLanguage } from "@codemirror/lang-markdown"
import { languages } from "@codemirror/language-data"
import {
  foldGutter,
  foldKeymap,
  indentOnInput,
} from "@codemirror/fold"
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands"
import {
  syntaxHighlighting,
  defaultHighlightStyle,
  bracketMatching,
  closeBrackets,
  highlightActiveLine,
  highlightSpecialChars,
  drawSelection,
} from "@codemirror/view"
import { searchKeymap } from "@codemirror/search"
import * as Y from "yjs"
import { yCollab } from "y-codemirror.next"
import type { AwarenessProtocol } from "@/lib/yjs-provider"
import { essayTheme } from "@/lib/codemirror/theme"
import { livePreview } from "@/lib/codemirror/live-preview"

interface EssayEditorProps {
  ydoc: Y.Doc
  ytext: Y.Text
  awareness: AwarenessProtocol | null
  readOnly?: boolean
  onChange?: (content: string) => void
  onCursorMove?: (line: number, col: number) => void
}

export function EssayEditor({
  ydoc,
  ytext,
  awareness,
  readOnly = false,
  onChange,
  onCursorMove,
}: EssayEditorProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)
  const undoManagerRef = useRef<Y.UndoManager | null>(null)

  useEffect(() => {
    if (!containerRef.current) return

    // Create undo manager (scoped to local edits)
    const undoManager = new Y.UndoManager(ytext, {
      trackedOrigins: new Set([null]), // null = local origin
    })
    undoManagerRef.current = undoManager

    // Build extensions
    const extensions = [
      // Markdown language
      markdown({
        base: markdownLanguage,
        codeLanguages: languages,
        addKeymap: true,
      }),

      // Folding
      foldGutter({
        markerDOM: (open: boolean) => {
          const span = document.createElement("span")
          span.textContent = open ? "▾" : "▸"
          span.style.cursor = "pointer"
          span.style.padding = "0 2px"
          return span
        },
      }),
      keymap.of(foldKeymap),

      // Core editor features
      EditorView.lineWrapping,
      highlightSpecialChars(),
      history(),
      drawSelection(),
      bracketMatching(),
      closeBrackets(),
      highlightActiveLine(),
      indentOnInput(),
      placeholder("Start writing your essay..."),

      // Keybindings
      keymap.of([
        ...defaultKeymap,
        ...historyKeymap,
        ...searchKeymap,
      ]),

      // Dark theme
      essayTheme,
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),

      // Yjs collaboration
      yCollab(ytext, awareness as any, {
        undoManager: undoManager as any,
      }),

      // Live Preview
      livePreview(),

      // Read-only
      EditorState.readOnly.of(readOnly),
      EditorView.editable.of(!readOnly),

      // Change listener
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          const content = update.state.doc.toString()
          onChange?.(content)
        }
        if (update.selectionSet) {
          const pos = update.state.selection.main.head
          const line = update.state.doc.lineAt(pos)
          onCursorMove?.(line.number, pos - line.from + 1)
        }
      }),
    ]

    const view = new EditorView({
      state: EditorState.create({
        doc: ytext.toString(),
        extensions,
      }),
      parent: containerRef.current,
    })

    viewRef.current = view

    return () => {
      view.destroy()
      viewRef.current = null
    }
  }, [])

  // Sync readOnly changes
  useEffect(() => {
    viewRef.current?.dispatch({
      effects: EditorView.editable.reconfigure(
        EditorView.editable.combined(
          viewRef.current.state.facet(EditorView.editable),
          !readOnly,
        ),
      ),
    })
  }, [readOnly])

  return (
    <div
      ref={containerRef}
      className="h-full w-full overflow-hidden"
    />
  )
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit components/essay/essay-editor.tsx
```

- [ ] **Step 3: Commit**

```bash
git add components/essay/essay-editor.tsx
git commit -m "feat: add CodeMirror 6 essay editor with Live Preview and Yjs"
```

---

## Task 8: Essay TopBar, Sidebar, StatusBar

**Files:**
- Create: `components/essay/essay-topbar.tsx`
- Create: `components/essay/essay-sidebar.tsx`
- Create: `components/essay/essay-statusbar.tsx`

- [ ] **Step 1: Create TopBar**

```typescript
// components/essay/essay-topbar.tsx
"use client"

import { useState } from "react"
import { useRouter } from "next/navigation"
import { ArrowLeft, PencilLine } from "lucide-react"
import { cn } from "@/lib/utils"

type SyncState = "synced" | "saving" | "offline"

interface EssayTopBarProps {
  title: string
  projectId: string
  syncState: SyncState
  collaborators: Array<{ name: string; color: string }>
  onRename: (title: string) => void
}

const syncConfig: Record<
  SyncState,
  { dot: string; label: string }
> = {
  synced: { dot: "bg-green-500", label: "Synced" },
  saving: { dot: "bg-yellow-500", label: "Saving..." },
  offline: { dot: "bg-red-500", label: "Offline" },
}

export function EssayTopBar({
  title,
  projectId,
  syncState,
  collaborators,
  onRename,
}: EssayTopBarProps) {
  const router = useRouter()
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(title)
  const config = syncConfig[syncState]

  return (
    <div className="flex h-10 items-center gap-3 border-b border-[#2a2a2a] bg-[#0f0f0f] px-4 shrink-0">
      {/* Back */}
      <button
        onClick={() => router.push(`/projects/${projectId}`)}
        className="flex items-center gap-1 text-xs text-[#888] hover:text-[#e0e0e0] transition-colors"
        title="Back to project (Ctrl+Shift+E)"
      >
        <ArrowLeft className="h-4 w-4" />
      </button>

      {/* Title */}
      {editing ? (
        <input
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            setEditing(false)
            if (draft.trim() && draft !== title) {
              onRename(draft.trim())
            } else {
              setDraft(title)
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur()
            if (e.key === "Escape") {
              setDraft(title)
              setEditing(false)
            }
          }}
          className="bg-[#1a1a1a] text-sm text-[#e0e0e0] border border-[#444] rounded px-2 py-0.5 outline-none focus:border-[#666] min-w-[200px]"
        />
      ) : (
        <button
          onClick={() => {
            setDraft(title)
            setEditing(true)
          }}
          className="flex items-center gap-1.5 text-sm text-[#e0e0e0] hover:text-white group"
        >
          <span className="font-medium">{title}</span>
          <PencilLine className="h-3 w-3 text-[#555] opacity-0 group-hover:opacity-100 transition-opacity" />
        </button>
      )}

      <div className="flex-1" />

      {/* Sync indicator */}
      <div className="flex items-center gap-1.5 text-xs text-[#666]">
        <span
          className={cn("inline-block h-2 w-2 rounded-full", config.dot)}
        />
        <span>{config.label}</span>
      </div>

      {/* Collaborator avatars */}
      {collaborators.length > 0 && (
        <div className="flex items-center -space-x-1.5">
          {collaborators.slice(0, 5).map((c, i) => (
            <div
              key={i}
              className="flex h-6 w-6 items-center justify-center rounded-full text-[10px] font-semibold text-white border border-[#2a2a2a]"
              style={{ backgroundColor: c.color }}
              title={c.name}
            >
              {c.name
                .split(" ")
                .map((s) => s[0])
                .join("")
                .slice(0, 2)
                .toUpperCase()}
            </div>
          ))}
          {collaborators.length > 5 && (
            <div className="flex h-6 w-6 items-center justify-center rounded-full bg-[#333] text-[10px] text-[#aaa] border border-[#2a2a2a]">
              +{collaborators.length - 5}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step 2: Create Sidebar**

```typescript
// components/essay/essay-sidebar.tsx
"use client"

import { useEffect, useState, useCallback } from "react"
import { useRouter } from "next/navigation"
import { Folder, FolderOpen, FileCode, FileText, RefreshCw } from "lucide-react"
import { cn } from "@/lib/utils"
import type { FileTreeItem } from "@/lib/tauri-api"
import * as tauriApi from "@/lib/tauri-api"

interface EssaySidebarProps {
  projectId: string
  fileId: string // current file ID, to highlight in tree
}

function fileIcon(file: FileTreeItem) {
  const ext = file.name.split(".").pop()?.toLowerCase()
  if (ext === "md") return <FileText className="h-4 w-4 text-[#d4a574]" />
  return <FileCode className="h-4 w-4 text-[#64b5f6]" />
}

export function EssaySidebar({
  projectId,
  fileId,
}: EssaySidebarProps) {
  const router = useRouter()
  const [tree, setTree] = useState<FileTreeItem | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set(["/"]))

  useEffect(() => {
    // Load file tree — use Tauri or server API
    if (tauriApi.isTauri()) {
      tauriApi.listFiles().then(setTree).catch(() => setTree(null))
    }
    // Server mode tree loading handled by parent page if needed
  }, [projectId])

  const toggleExpand = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }, [])

  const handleFileClick = useCallback(
    (file: FileTreeItem) => {
      if (file.id === fileId) return // already open
      const ext = file.name.split(".").pop()?.toLowerCase()
      if (ext === "md") {
        // Navigate to another essay
        if (file.id) {
          router.push(
            `/projects/${projectId}/essay?file=${file.id}`,
          )
        }
      } else {
        // Navigate back to project with this file
        router.push(`/projects/${projectId}`)
      }
    },
    [projectId, fileId, router],
  )

  return (
    <div className="flex flex-col h-full bg-[#0d0d0d] border-l border-[#2a2a2a]">
      {/* Header */}
      <div className="flex items-center justify-between px-3 h-8 border-b border-[#2a2a2a] shrink-0">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-[#666]">
          Project Files
        </span>
        <button
          onClick={() =>
            tauriApi.listFiles().then(setTree).catch(() => {})
          }
          className="text-[#555] hover:text-[#888]"
          title="Refresh file tree"
        >
          <RefreshCw className="h-3 w-3" />
        </button>
      </div>

      {/* Tree */}
      <div className="flex-1 overflow-y-auto py-1">
        {tree ? (
          <FileTreeRenderer
            tree={tree}
            depth={0}
            activeFileId={fileId}
            expanded={expanded}
            onToggle={toggleExpand}
            onFileClick={handleFileClick}
          />
        ) : (
          <div className="px-3 py-2 text-xs text-[#555]">
            Loading...
          </div>
        )}
      </div>
    </div>
  )
}

// ─── Recursive Tree Renderer ──────────────────────────

function FileTreeRenderer({
  tree,
  depth,
  activeFileId,
  expanded,
  onToggle,
  onFileClick,
}: {
  tree: FileTreeItem
  depth: number
  activeFileId: string
  expanded: Set<string>
  onToggle: (path: string) => void
  onFileClick: (file: FileTreeItem) => void
}) {
  const isFolder = tree.type === "folder"
  const isExpanded = expanded.has(tree.path)
  const isActive = tree.id === activeFileId

  return (
    <div>
      <button
        className={cn(
          "flex h-7 w-full items-center gap-2 px-2 text-left text-xs text-[#b4b4b4] hover:bg-[#1a1a1a]",
          isActive && "bg-[#1e1e2e] text-[#e0e0e0]",
        )}
        style={{ paddingLeft: depth * 12 + 8 }}
        onClick={() =>
          isFolder ? onToggle(tree.path) : onFileClick(tree)
        }
      >
        {isFolder ? (
          isExpanded ? (
            <FolderOpen className="h-4 w-4 text-[#d4a574]" />
          ) : (
            <Folder className="h-4 w-4 text-[#d4a574]" />
          )
        ) : (
          fileIcon(tree)
        )}
        <span className="truncate">{tree.name}</span>
      </button>
      {isFolder && isExpanded && tree.children && (
        <div>
          {tree.children.map((child) => (
            <FileTreeRenderer
              key={child.path || child.name}
              tree={child}
              depth={depth + 1}
              activeFileId={activeFileId}
              expanded={expanded}
              onToggle={onToggle}
              onFileClick={onFileClick}
            />
          ))}
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step 3: Create StatusBar**

```typescript
// components/essay/essay-statusbar.tsx
"use client"

interface EssayStatusBarProps {
  wordCount: number
  sectionIndex: number
  sectionCount: number
  lastSaved: Date | null
}

export function EssayStatusBar({
  wordCount,
  sectionIndex,
  sectionCount,
  lastSaved,
}: EssayStatusBarProps) {
  const savedText = lastSaved
    ? `Saved · ${formatTimeAgo(lastSaved)}`
    : "Unsaved"

  return (
    <div className="flex h-7 items-center gap-4 border-t border-[#2a2a2a] bg-[#0f0f0f] px-4 text-xs text-[#555] shrink-0 select-none">
      <span>
        {wordCount.toLocaleString()} words
      </span>
      <span className="text-[#444]">|</span>
      <span>
        § {sectionIndex} of {sectionCount}
      </span>
      <span className="flex-1" />
      <span className="flex items-center gap-1.5">
        <span className="inline-block h-1.5 w-1.5 rounded-full bg-green-600" />
        {savedText}
      </span>
    </div>
  )
}

function formatTimeAgo(date: Date): string {
  const seconds = Math.floor((Date.now() - date.getTime()) / 1000)
  if (seconds < 10) return "just now"
  if (seconds < 60) return `${seconds}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ago`
}
```

- [ ] **Step 4: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit components/essay/essay-topbar.tsx components/essay/essay-sidebar.tsx components/essay/essay-statusbar.tsx
```

- [ ] **Step 5: Commit**

```bash
git add components/essay/essay-topbar.tsx components/essay/essay-sidebar.tsx components/essay/essay-statusbar.tsx
git commit -m "feat: add essay TopBar, Sidebar, and StatusBar components"
```

---

## Task 9: Essay Page Route

**Files:**
- Create: `app/projects/[id]/essay/page.tsx`

- [ ] **Step 1: Create the essay route page**

```typescript
// app/projects/[id]/essay/page.tsx
"use client"

import { useEffect, useState, useCallback, Suspense } from "react"
import { useParams, useRouter, useSearchParams } from "next/navigation"
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels"
import { EssayEditor } from "@/components/essay/essay-editor"
import { EssayTopBar } from "@/components/essay/essay-topbar"
import { EssaySidebar } from "@/components/essay/essay-sidebar"
import { EssayStatusBar } from "@/components/essay/essay-statusbar"
import { useEssayCollab } from "@/components/essay/use-essay-collab"
import { setLocalUserInfo } from "@/lib/codemirror/awareness"
import type { AwarenessUserInfo } from "@/lib/codemirror/awareness"
import { isTauri, listFiles } from "@/lib/tauri-api"
import { getToken } from "@/lib/api"

type SyncState = "synced" | "saving" | "offline"

function getOrCreateUserInfo(): AwarenessUserInfo {
  const key = "essay-user-info"
  if (typeof window !== "undefined") {
    const stored = localStorage.getItem(key)
    if (stored) {
      try {
        return JSON.parse(stored)
      } catch {
        // fall through
      }
    }
  }
  const colors = [
    "#f87171", "#60a5fa", "#34d399", "#fbbf24",
    "#a78bfa", "#f472b6", "#38bdf8", "#fb923c",
  ]
  const color = colors[Math.floor(Math.random() * colors.length)]
  const info: AwarenessUserInfo = {
    name: "User",
    color,
    colorLight: color + "33",
  }
  if (typeof window !== "undefined") {
    localStorage.setItem(key, JSON.stringify(info))
  }
  return info
}

function EssayPageContent() {
  const params = useParams<{ id: string }>()
  const searchParams = useSearchParams()
  const router = useRouter()

  const projectId = params.id
  const fileId = searchParams.get("file")

  const [title, setTitle] = useState("Untitled Essay")
  const [syncState, setSyncState] = useState<SyncState>("offline")
  const [wordCount, setWordCount] = useState(0)
  const [sectionInfo, setSectionInfo] = useState({
    index: 0,
    count: 0,
  })
  const [lastSaved, setLastSaved] = useState<Date | null>(null)
  const [collaborators, setCollaborators] = useState<
    Array<{ name: string; color: string }>
  >([])
  const [notFound, setNotFound] = useState(false)

  const userInfo = getOrCreateUserInfo()
  const token = getToken()

  // Load file info
  useEffect(() => {
    if (!fileId) {
      setNotFound(true)
      return
    }

    // Try to load file name from Tauri
    if (isTauri()) {
      listFiles()
        .then((tree) => {
          const findFile = (
            node: typeof tree,
          ): { name: string } | null => {
            if ((node as any).id === fileId) {
              return { name: node.name }
            }
            if (node.children) {
              for (const c of node.children) {
                const found = findFile(c)
                if (found) return found
              }
            }
            return null
          }
          const found = findFile(tree)
          if (found) {
            setTitle(found.name.replace(/\.md$/, ""))
          }
        })
        .catch(() => {})
    }

    setNotFound(false)
  }, [fileId])

  // Set up collaboration
  const { ydoc, ytext, awareness } = useEssayCollab({
    fileId: fileId ?? "",
    readOnly: false,
    onSynced: () => setSyncState("synced"),
  })

  // Set local user info once awareness is ready
  useEffect(() => {
    if (awareness) {
      setLocalUserInfo(awareness, userInfo)

      // Poll awareness states for collaborator list
      const interval = setInterval(() => {
        const states = awareness.getStates()
        const others: Array<{ name: string; color: string }> = []
        states.forEach((state, clientId) => {
          if (clientId !== awareness.doc?.clientID) {
            const user = state.user as AwarenessUserInfo | undefined
            if (user) {
              others.push({ name: user.name, color: user.color })
            }
          }
        })
        setCollaborators(others)
      }, 2000)

      return () => clearInterval(interval)
    }
  }, [awareness, userInfo])

  // Compute word count and section info
  const handleContentChange = useCallback((content: string) => {
    // Word count (markdown-aware: strip syntax characters roughly)
    const words = content
      .replace(/[#*`~>\[\]()!_|]/g, " ")
      .replace(/\$\$[\s\S]*?\$\$/g, " ")
      .replace(/\$[^$]*\$/g, " ")
      .split(/\s+/)
      .filter(Boolean)
    setWordCount(words.length)

    // Section count
    const headings = content.match(/^#{1,6}\s/gm)
    setSectionInfo({
      index: 0, // simplified; could compute cursor position
      count: headings ? headings.length + 1 : 1,
    })

    // Debounced save indicator
    setSyncState("saving")
    const timer = setTimeout(() => {
      setSyncState("synced")
      setLastSaved(new Date())
    }, 1000)
    return () => clearTimeout(timer)
  }, [])

  const handleRename = useCallback(
    (newTitle: string) => {
      setTitle(newTitle)
      // Title displayed in TopBar updates immediately; server-side rename
      // (file metadata update) is deferred to a follow-up task since it
      // requires project-specific API endpoints beyond this plan's scope.
      console.log("[essay] rename requested:", newTitle)
    },
    [],
  )

  // Handle keyboard shortcut to exit
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key === "E") {
        e.preventDefault()
        router.push(`/projects/${projectId}`)
      }
    }
    window.addEventListener("keydown", handler)
    return () => window.removeEventListener("keydown", handler)
  }, [projectId, router])

  // Not found state
  if (notFound || !fileId) {
    return (
      <div className="flex h-screen items-center justify-center bg-[#0d0d0d]">
        <div className="text-center">
          <h1 className="text-2xl font-semibold text-[#e0e0e0] mb-2">
            File not found
          </h1>
          <p className="text-sm text-[#666] mb-4">
            The requested essay file does not exist or you do not have access.
          </p>
          <button
            onClick={() => router.push(`/projects/${projectId}`)}
            className="text-sm text-[#569cd6] hover:underline"
          >
            ← Back to project
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-screen flex-col bg-[#0d0d0d] overflow-hidden">
      <EssayTopBar
        title={title}
        projectId={projectId}
        syncState={syncState}
        collaborators={collaborators}
        onRename={handleRename}
      />

      <PanelGroup direction="horizontal" className="flex-1">
        {/* Editor Panel */}
        <Panel defaultSize={70} minSize={40}>
          <EssayEditor
            ydoc={ydoc}
            ytext={ytext}
            awareness={awareness}
            readOnly={!token}
            onChange={handleContentChange}
          />
        </Panel>

        <PanelResizeHandle className="w-1 bg-[#2a2a2a] hover:bg-[#444] transition-colors active:bg-[#569cd6]" />

        {/* Sidebar Panel */}
        <Panel defaultSize={30} minSize={15} maxSize={40}>
          <EssaySidebar projectId={projectId} fileId={fileId} />
        </Panel>
      </PanelGroup>

      <EssayStatusBar
        wordCount={wordCount}
        sectionIndex={sectionInfo.index}
        sectionCount={sectionInfo.count}
        lastSaved={lastSaved}
      />
    </div>
  )
}

// Wrap in Suspense for useSearchParams
export default function EssayPage() {
  return (
    <Suspense
      fallback={
        <div className="flex h-screen items-center justify-center bg-[#0d0d0d]">
          <div className="text-sm text-[#666]">Loading...</div>
        </div>
      }
    >
      <EssayPageContent />
    </Suspense>
  )
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit app/projects/[id]/essay/page.tsx
```

- [ ] **Step 3: Commit**

```bash
git add app/projects/
git commit -m "feat: add essay page route with editor, sidebar, and status bar"
```

---

## Task 10: Wire .md Files to Essay Editor

**Files:**
- Modify: `components/layout/modeler-workbench.tsx`

- [ ] **Step 1: Add essay navigation in openFile**

In the `openFile` function (around line 1509), add markdown routing before the tab creation logic:

```typescript
// Find the openFile function and add at the top (after computing `lang`):
const openFile = async (file: FileTreeItem) => {
  const pathKey = normalizePathKey(file.path)
  const hostManifestEntry = workspaceMode === "host" ? loadSyncManifest(projectId)[pathKey] : undefined
  const remoteContent = workspaceMode === "guest" && file.id
    ? await getProjectFileContent(projectId, file.id)
    : null
  const lang = fileLanguage(file)

  // NEW: Route .md files to essay editor
  if (lang === "markdown" && file.id) {
    router.push(`/projects/${projectId}/essay?file=${file.id}`)
    return
  }

  // ... rest of existing openFile logic
```

- [ ] **Step 2: Verify the logic compiles**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit
```

- [ ] **Step 3: Commit**

```bash
git add components/layout/modeler-workbench.tsx
git commit -m "feat: route .md files to essay editor instead of code editor"
```

---

## Task 11: Backend — Extend SyncRoom with Awareness Channel

**Files:**
- Modify: `server/src/sync/room.rs`

- [ ] **Step 1: Add awareness_tx to SyncRoom and wire it**

Modify `SyncRoom` struct and `RoomRegistry::release`:

```rust
// In room.rs, modify the SyncRoom struct to add awareness_tx:

pub struct SyncRoom {
    pub file_id: String,
    pub doc: yrs::Doc,
    pub update_tx: broadcast::Sender<Vec<u8>>,
    pub awareness_tx: broadcast::Sender<Vec<u8>>,  // NEW
    pub clients: AtomicUsize,
}

impl SyncRoom {
    pub fn new(file_id: String) -> Self {
        let doc = yrs::Doc::new();
        let (update_tx, _) = broadcast::channel::<Vec<u8>>(256);
        let (awareness_tx, _) = broadcast::channel::<Vec<u8>>(256);  // NEW
        Self {
            file_id,
            doc,
            update_tx,
            awareness_tx,  // NEW
            clients: AtomicUsize::new(0),
        }
    }

    // ... apply_update, encode_state unchanged ...
}
```

- [ ] **Step 2: Verify with cargo check**

```bash
cd D:/5_user/mathmodel/server && cargo check
```

- [ ] **Step 3: Commit**

```bash
git add server/src/sync/room.rs
git commit -m "feat: add awareness broadcast channel to SyncRoom"
```

---

## Task 12: Backend — Handle Awareness in WebSocket Loop

**Files:**
- Modify: `server/src/sync/handlers.rs`

- [ ] **Step 1: Add awareness handling in handle_socket**

In the `handle_socket` function, add awareness subscribe/send inside the `tokio::select!` loop:

```rust
// In handle_socket, after update_rx subscription, add:

async fn handle_socket(
    mut socket: WebSocket,
    file_id: String,
    pool: sqlx::SqlitePool,
    registry: Arc<RoomRegistry>,
    can_write: bool,
) {
    let room = registry.get_or_create(&file_id, &pool).await;

    // Send current full state to new client
    {
        let r = room.read().await;
        let state = r.encode_state();
        let msg_full = SyncMessage::SyncFull { state };
        if let Ok(json) = serde_json::to_string(&msg_full) {
            let _ = socket.send(Message::Text(json.into())).await;
        }
    }

    // Subscribe to content updates from other clients
    let mut update_rx = room.read().await.update_tx.subscribe();
    // NEW: Subscribe to awareness updates
    let mut awareness_rx = room.read().await.awareness_tx.subscribe();

    loop {
        tokio::select! {
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<SyncMessage>(&text) {
                            Ok(SyncMessage::SyncUpdate { update }) => {
                                if !can_write {
                                    continue;
                                }
                                let mut r = room.write().await;
                                if r.apply_update(&update).is_ok() {
                                    let _ = r.update_tx.send(update);
                                }
                            }
                            // NEW: Handle incoming awareness
                            Ok(SyncMessage::Awareness { state }) => {
                                let r = room.read().await;
                                let _ = r.awareness_tx.send(state);
                            }
                            _ => continue,
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => continue,
                }
            }
            Ok(update) = update_rx.recv() => {
                let msg = SyncMessage::SyncUpdate { update };
                if let Ok(json) = serde_json::to_string(&msg) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
            // NEW: Fan-out awareness to all clients
            Ok(awareness_update) = awareness_rx.recv() => {
                let msg = SyncMessage::Awareness { state: awareness_update };
                if let Ok(json) = serde_json::to_string(&msg) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
        }
    }

    // Persist CRDT state on disconnect
    registry.release(&file_id, &pool).await;
}
```

- [ ] **Step 2: Verify with cargo check**

```bash
cd D:/5_user/mathmodel/server && cargo check
```

- [ ] **Step 3: Commit**

```bash
git add server/src/sync/handlers.rs
git commit -m "feat: handle awareness messages in WebSocket sync loop"
```

---

## Task 13: Integration Test & Verification

**Files:**
- No new files — manual verification checklist

- [ ] **Step 1: Verify full build**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit
```

Expected: no new TypeScript errors.

```bash
cd D:/5_user/mathmodel/server && cargo check
cd D:/5_user/mathmodel && cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: both pass.

- [ ] **Step 2: Verify no regressions in existing tests**

```bash
cd D:/5_user/mathmodel/src-tauri && cargo test
```

Expected: all 53 tests pass (same as before).

- [ ] **Step 3: Manual verification checklist**

Open the Tauri app and verify:

1. **File tree routing**: Click a `.md` file → navigates to essay page (not code tab)
2. **Back navigation**: Click ← or press Ctrl+Shift+E → returns to project page
3. **Editor renders**: CodeMirror displays markdown content, dark theme applied
4. **Live Preview**: Move cursor to a heading line → `#` visible; click away → `#` hidden, heading large
5. **Folding**: Click ▸/▾ icon on a heading → section collapses/expands
6. **Sidebar**: File tree visible on right; clicking another `.md` switches essay; clicking non-`.md` returns to project
7. **Status bar**: Shows word count and section count
8. **Two-tab sync**: Open same essay in two browser tabs → content syncs in real-time
9. **Cursor sync**: Two tabs → each sees the other's cursor position (after Task 12 backend deployed)
10. **Offline handling**: Disconnect server → editor stays editable, indicator turns red; reconnect → sync resumes

- [ ] **Step 4: Commit any fixes found during verification**

```bash
git add -A
git commit -m "fix: integration fixes from essay editor verification"
```

---

## Task 14: Polish — LaTeX Rendering in Live Preview

**Files:**
- Modify: `lib/codemirror/live-preview.ts`

- [ ] **Step 1: Upgrade InlineMathWidget to render with KaTeX**

```typescript
// In live-preview.ts, replace the InlineMathWidget class:

class InlineMathWidget extends WidgetType {
  private rendered = false

  constructor(readonly latex: string) {
    super()
  }

  eq(other: InlineMathWidget) {
    return other.latex === this.latex
  }

  toDOM() {
    const span = document.createElement("span")
    span.className = "cm-latex-inline"

    // Try to render with KaTeX
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const katex = require("katex")
      katex.render(this.latex, span, {
        throwOnError: false,
        displayMode: false,
        output: "html",
        strict: false,
      })
      this.rendered = true
    } catch {
      // Fallback: show raw LaTeX
      span.textContent = `$${this.latex}$`
      span.style.color = "#d4a574"
      span.style.fontStyle = "italic"
    }

    return span
  }

  ignoreEvent() {
    return false
  }
}
```

- [ ] **Step 2: Add display math (block) support**

Add to the `buildDecorations` method, after the InlineMath block:

```typescript
// Render display math blocks ($$ ... $$)
if (nodeName === "DisplayMath" || nodeName === "MathBlock") {
  const mathText = doc.sliceString(from + 2, to - 2)
  builder.add(
    from,
    to,
    Decoration.replace({
      widget: new DisplayMathWidget(mathText),
      inclusive: true,
    }),
  )
  return
}
```

And add the widget:

```typescript
class DisplayMathWidget extends WidgetType {
  constructor(readonly latex: string) {
    super()
  }

  eq(other: DisplayMathWidget) {
    return other.latex === this.latex
  }

  toDOM() {
    const div = document.createElement("div")
    div.className = "cm-latex-display"

    try {
      const katex = require("katex")
      katex.render(this.latex, div, {
        throwOnError: false,
        displayMode: true,
        output: "html",
        strict: false,
      })
    } catch {
      div.textContent = `$$${this.latex}$$`
      div.style.color = "#d4a574"
    }

    return div
  }

  ignoreEvent() {
    return false
  }

  get estimatedHeight() {
    return 40
  }
}
```

- [ ] **Step 3: Verify TypeScript**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit lib/codemirror/live-preview.ts
```

- [ ] **Step 4: Commit**

```bash
git add lib/codemirror/live-preview.ts
git commit -m "feat: add KaTeX inline and display math rendering in Live Preview"
```
