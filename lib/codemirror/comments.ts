// lib/codemirror/comments.ts
import {
  ViewPlugin,
  Decoration,
  DecorationSet,
  type EditorView,
  type PluginValue,
  type ViewUpdate,
} from "@codemirror/view"
import { RangeSetBuilder } from "@codemirror/state"
import * as Y from "yjs"

// ─── Comment Data ────────────────────────────────────

export interface EssayComment {
  id: string
  text: string
  /** Absolute character offset in the document */
  from: number
  /** Absolute character offset in the document */
  to: number
  author: string
  color: string
  createdAt: number
}

export function createComment(
  id: string,
  text: string,
  from: number,
  to: number,
  author: string,
  color: string,
): EssayComment {
  return { id, text, from, to, author, color, createdAt: Date.now() }
}

// ─── Yjs Comments Map Helpers ────────────────────────

export function getCommentsMap(ydoc: Y.Doc): Y.Map<EssayComment> {
  return ydoc.getMap<EssayComment>("comments")
}

export function addComment(
  map: Y.Map<EssayComment>,
  comment: EssayComment,
) {
  map.set(comment.id, comment as never as EssayComment)
}

export function deleteComment(map: Y.Map<EssayComment>, id: string) {
  map.delete(id)
}

export function getCommentsArray(map: Y.Map<EssayComment>): EssayComment[] {
  return Array.from(map.values())
}

// ─── Colors ──────────────────────────────────────────

const AUTHOR_COLORS = [
  "#f87171", "#60a5fa", "#34d399", "#fbbf24",
  "#a78bfa", "#f472b6", "#38bdf8", "#fb923c",
]

export function assignCommentColor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash)
  }
  return AUTHOR_COLORS[Math.abs(hash) % AUTHOR_COLORS.length]
}

// ─── Floating Input ──────────────────────────────────

function showCommentInput(
  view: EditorView,
  onSubmit: (text: string) => void,
) {
  const coords = view.coordsAtPos(view.state.selection.main.head)
  if (!coords) return

  const container = document.createElement("div")
  container.className = "cm-comment-input"
  Object.assign(container.style, {
    position: "absolute",
    left: `${coords.left}px`,
    top: `${coords.bottom + 6}px`,
    zIndex: "100",
    backgroundColor: "#1e1e1e",
    border: "1px solid #444",
    borderRadius: "6px",
    padding: "8px",
    minWidth: "280px",
    boxShadow: "0 4px 16px rgba(0,0,0,0.5)",
    fontFamily: "sans-serif",
  })

  const input = document.createElement("textarea")
  input.placeholder = "Add comment..."
  Object.assign(input.style, {
    width: "100%",
    minHeight: "48px",
    backgroundColor: "#2a2a2a",
    color: "#e0e0e0",
    border: "1px solid #444",
    borderRadius: "4px",
    padding: "6px 8px",
    fontSize: "13px",
    resize: "vertical",
    fontFamily: "sans-serif",
  })
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault()
      const text = input.value.trim()
      if (text) onSubmit(text)
      container.remove()
    }
  })

  const buttons = document.createElement("div")
  buttons.style.cssText =
    "display:flex;justify-content:flex-end;gap:6px;margin-top:6px;"

  const cancelBtn = document.createElement("button")
  cancelBtn.textContent = "Cancel"
  cancelBtn.style.cssText =
    "background:#3a3a3a;color:#ccc;border:none;border-radius:3px;padding:4px 10px;font-size:12px;cursor:pointer;"

  const submitBtn = document.createElement("button")
  submitBtn.textContent = "Add (Ctrl+↵)"
  submitBtn.style.cssText =
    "background:#569cd6;color:white;border:none;border-radius:3px;padding:4px 10px;font-size:12px;cursor:pointer;"

  cancelBtn.addEventListener("click", () => container.remove())
  submitBtn.addEventListener("click", () => {
    const text = input.value.trim()
    if (text) onSubmit(text)
    container.remove()
  })

  buttons.appendChild(cancelBtn)
  buttons.appendChild(submitBtn)
  container.appendChild(input)
  container.appendChild(buttons)

  const scrollParent = view.scrollDOM.parentElement
  if (scrollParent) scrollParent.appendChild(container)
  else document.body.appendChild(container)
  input.focus()

  setTimeout(() => {
    const outside = (e: MouseEvent) => {
      if (!container.contains(e.target as Node)) {
        container.remove()
        document.removeEventListener("click", outside)
      }
    }
    document.addEventListener("click", outside)
  }, 0)
}

// ─── Plugin ──────────────────────────────────────────

class CommentsPlugin implements PluginValue {
  decorations: DecorationSet
  private commentsMap: Y.Map<EssayComment>
  private toolbarEl: HTMLDivElement | null = null

  constructor(
    view: EditorView,
    commentsMap: Y.Map<EssayComment>,
  ) {
    this.commentsMap = commentsMap
    this.decorations = this.buildDecorations(view)
    this.setupToolbar(view)

    commentsMap.observe(() => {
      this.decorations = this.buildDecorations(view)
      view.dispatch({})
    })
  }

  update(update: ViewUpdate) {
    if (
      update.docChanged ||
      update.selectionSet ||
      update.viewportChanged
    ) {
      this.decorations = this.buildDecorations(update.view)
      this.updateToolbar(update.view)
    }
  }

  private buildDecorations(view: EditorView): DecorationSet {
    const builder = new RangeSetBuilder<Decoration>()
    const docLen = view.state.doc.length
    const comments = getCommentsArray(this.commentsMap)

    for (const c of comments) {
      // Clamp positions to doc bounds
      const from = Math.max(0, Math.min(c.from, docLen))
      const to = Math.max(from + 1, Math.min(c.to, docLen))
      if (from >= to) continue

      builder.add(
        from,
        to,
        Decoration.mark({
          attributes: {
            "data-comment-id": c.id,
            style: `background-color:${c.color}33;border-bottom:2px solid ${c.color};cursor:pointer;border-radius:1px;`,
            title: `${c.author}: ${c.text.slice(0, 100)}`,
          },
        }),
      )
    }

    return builder.finish()
  }

  // ─── Toolbar ──────────────────────────────────────

  private setupToolbar(view: EditorView) {
    this.toolbarEl = document.createElement("div")
    this.toolbarEl.className = "cm-comment-toolbar"
    Object.assign(this.toolbarEl.style, {
      position: "absolute",
      zIndex: "99",
      display: "none",
      backgroundColor: "#1e1e1e",
      border: "1px solid #444",
      borderRadius: "4px",
      padding: "2px 4px",
      boxShadow: "0 2px 8px rgba(0,0,0,0.4)",
      fontFamily: "sans-serif",
    })

    const btn = document.createElement("button")
    btn.textContent = "💬 Comment"
    Object.assign(btn.style, {
      backgroundColor: "transparent",
      color: "#e0e0e0",
      border: "none",
      borderRadius: "3px",
      padding: "4px 8px",
      fontSize: "12px",
      cursor: "pointer",
      whiteSpace: "nowrap",
    })
    btn.addEventListener("mousedown", (e) => {
      e.preventDefault()
      e.stopPropagation()
      const sel = view.state.selection.main
      if (!sel.empty) {
        this.startComment(view, sel.from, sel.to)
      }
      this.toolbarEl!.style.display = "none"
    })

    this.toolbarEl.appendChild(btn)
    const scrollParent = view.scrollDOM.parentElement
    if (scrollParent) scrollParent.appendChild(this.toolbarEl)
  }

  private updateToolbar(view: EditorView) {
    if (!this.toolbarEl) return
    const sel = view.state.selection.main
    if (sel.empty) {
      this.toolbarEl.style.display = "none"
      return
    }
    // coordsAtPos cannot be called during a CM update — defer to next frame
    requestAnimationFrame(() => {
      const headCoords = view.coordsAtPos(sel.head)
      if (!headCoords) {
        this.toolbarEl!.style.display = "none"
        return
      }
      const scrollParent = view.scrollDOM.parentElement
      if (!scrollParent) return
      const editorRect = scrollParent.getBoundingClientRect()
      this.toolbarEl!.style.display = "block"
      this.toolbarEl!.style.left = `${headCoords.left - editorRect.left}px`
      this.toolbarEl!.style.top = `${headCoords.top - editorRect.top - 34}px`
    })
  }

  private startComment(view: EditorView, from: number, to: number) {
    showCommentInput(view, (text) => {
      const existing = getCommentsArray(this.commentsMap)
      const count = existing.length + 1
      const id = `c-${Date.now()}-${count}`

      const comment = createComment(
        id,
        text,
        from,
        to,
        "User",
        assignCommentColor("User"),
      )
      addComment(this.commentsMap, comment)
      this.decorations = this.buildDecorations(view)
      view.dispatch({})
    })
  }

  destroy() {
    this.toolbarEl?.remove()
    this.toolbarEl = null
  }
}

// ─── Export ───────────────────────────────────────────

export function commentsPlugin(commentsMap: Y.Map<EssayComment>) {
  return ViewPlugin.define(
    (view) => new CommentsPlugin(view, commentsMap),
    { decorations: (plugin) => plugin.decorations },
  )
}
