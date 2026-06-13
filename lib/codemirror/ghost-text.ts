// lib/codemirror/ghost-text.ts
// Inspired by Tabby's codeCompletion module — ghost text inline AI suggestions.
import {
  ViewPlugin,
  Decoration,
  DecorationSet,
  WidgetType,
  keymap,
  type EditorView,
  type PluginValue,
  type ViewUpdate,
} from "@codemirror/view"
import { EditorSelection, type TransactionSpec } from "@codemirror/state"

// ─── Ghost Widget ────────────────────────────────────

const GHOST_CSS =
  "color:rgba(255,255,255,0.28);font-style:italic;pointer-events:none;white-space:pre-wrap;"

// Cap the suggestion so it stays a short hint ("一段"), not a whole essay.
// Once streaming output passes this, we abort and trim to a clean boundary.
const MAX_GHOST_CHARS = 280

// How long to wait after the user stops typing before requesting a ghost.
const GHOST_DEBOUNCE_MS = 1500

class GhostWidget extends WidgetType {
  constructor(readonly text: string) {
    super()
  }

  eq(other: GhostWidget): boolean {
    return other.text === this.text
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span")
    span.style.cssText = GHOST_CSS
    span.textContent = this.text
    span.setAttribute("aria-hidden", "true")
    return span
  }

  ignoreEvent(): boolean {
    return true
  }
}

// ─── State ───────────────────────────────────────────

export interface GhostState {
  /** Current ghost suggestion text (empty = none) */
  text: string
  /** Cursor position where the ghost is anchored */
  anchor: number
  /** Whether a request is in-flight */
  loading: boolean
}

export const EMPTY_GHOST: GhostState = { text: "", anchor: -1, loading: false }

// Trim an over-long suggestion back to the last clean sentence boundary, so a
// capped stream ends mid-thought as little as possible.
function trimToHint(text: string): string {
  const capped = text.slice(0, MAX_GHOST_CHARS)
  const lastBoundary = Math.max(
    capped.lastIndexOf("."),
    capped.lastIndexOf("。"),
    capped.lastIndexOf("\n"),
    capped.lastIndexOf("！"),
    capped.lastIndexOf("？"),
    capped.lastIndexOf("!"),
    capped.lastIndexOf("?"),
  )
  // Only trim at a boundary if it keeps a worthwhile amount of text.
  return lastBoundary > MAX_GHOST_CHARS * 0.5
    ? capped.slice(0, lastBoundary + 1)
    : capped
}

export type GhostFetcher = (
  prefix: string,
  suffix: string,
  signal: AbortSignal,
  onToken: (token: string) => void,
  onDone: () => void,
) => void

// ─── Plugin ──────────────────────────────────────────

class GhostTextPlugin implements PluginValue {
  decorations: DecorationSet
  ghost: GhostState = { ...EMPTY_GHOST }
  private view: EditorView | null = null
  private debounceTimer: ReturnType<typeof setTimeout> | null = null
  private abortController: AbortController | null = null
  private fetcher: GhostFetcher | null = null

  get currentGhost(): GhostState {
    return this.ghost
  }

  /** Call from outside to configure the AI fetcher */
  configure(fetcher: GhostFetcher) {
    this.fetcher = fetcher
    // Do NOT trigger a request here. On open the document is near-empty (and
    // still settling from collaborative sync), so an immediate "continue the
    // essay" prompt makes the model generate a whole essay from nothing. Wait
    // for the user to actually type — update() drives all scheduling.
  }

  constructor(view: EditorView) {
    this.decorations = Decoration.none
    this.view = view
  }

  update(update: ViewUpdate) {
    if (!update.docChanged) return

    // Only react to genuine local user input. Remote collaborative sync (yjs)
    // and our own empty refresh dispatches also fire docChanged; reacting to
    // those would trigger ghost requests on open and feed back on every token.
    const isLocalUserEdit = update.transactions.some(
      (tr) => tr.isUserEvent("input") || tr.isUserEvent("delete"),
    )
    if (!isLocalUserEdit) return

    // A user edit invalidates any pending suggestion. Clear the previous timer
    // so rapid typing collapses to a single request instead of stacking one
    // per keystroke.
    if (this.debounceTimer) clearTimeout(this.debounceTimer)
    // dispatch() not allowed during update — defer to next frame
    requestAnimationFrame(() => this.dismissGhost())
    this.debounceTimer = setTimeout(
      () => this.scheduleAfterTyping(),
      GHOST_DEBOUNCE_MS,
    )
  }

  private scheduleAfterTyping() {
    if (!this.fetcher || !this.view) return
    const { state } = this.view
    const pos = state.selection.main.head
    const doc = state.doc

    const prefix = doc.sliceString(Math.max(0, pos - 500), pos)
    const suffix = doc.sliceString(pos, Math.min(doc.length, pos + 200))

    // Need enough leading text for a meaningful continuation. On a near-empty
    // doc "continue the essay" has nothing to anchor on and the model invents
    // an entire essay — exactly the runaway we want to avoid.
    if (prefix.trim().length < 8) return

    // Cancel previous
    this.abortController?.abort()
    this.abortController = new AbortController()
    const signal = this.abortController.signal

    this.ghost = { text: "", anchor: pos, loading: true }
    this.refreshDecorations()

    this.fetcher(
      prefix,
      suffix,
      signal,
      (token: string) => {
        if (signal.aborted) return
        const nextText = this.ghost.text + token
        this.ghost = {
          ...this.ghost,
          text: nextText,
          loading: false,
        }
        this.refreshDecorations()
        // Keep the suggestion to a short hint: once we have enough, stop the
        // stream rather than letting the model write an entire essay inline.
        if (nextText.length >= MAX_GHOST_CHARS) {
          this.abortController?.abort()
          this.ghost = { ...this.ghost, text: trimToHint(nextText), loading: false }
          this.refreshDecorations()
        }
      },
      () => {
        if (signal.aborted) return
        this.ghost = { ...this.ghost, loading: false }
        // Post-process: drop if too short or duplicates suffix
        if (
          this.ghost.text.length < 3 ||
          suffix.startsWith(this.ghost.text)
        ) {
          this.dismissGhost()
        }
        this.refreshDecorations()
      },
    )
  }

  private refreshDecorations() {
    if (!this.view || this.ghost.anchor < 0 || !this.ghost.text) {
      this.decorations = Decoration.none
    } else {
      const widget = Decoration.widget({
        widget: new GhostWidget(this.ghost.text),
        side: 0,
      })
      this.decorations = Decoration.set([
        widget.range(this.ghost.anchor),
      ])
    }
    this.view?.dispatch({})
  }

  dismissGhost() {
    this.abortController?.abort()
    this.abortController = null
    this.ghost = { ...EMPTY_GHOST }
    this.refreshDecorations()
  }

  acceptGhost() {
    if (!this.view || !this.ghost.text) return false
    const pos = this.ghost.anchor
    const text = this.ghost.text
    this.dismissGhost()
    this.view.dispatch({
      changes: { from: pos, insert: text },
      selection: EditorSelection.cursor(pos + text.length),
    })
    return true
  }

  destroy() {
    this.abortController?.abort()
    if (this.debounceTimer) clearTimeout(this.debounceTimer)
  }
}

// ─── Export ───────────────────────────────────────────

export function ghostTextPlugin() {
  const plugin = ViewPlugin.fromClass(GhostTextPlugin, {
    decorations: (p) => p.decorations,
  })

  return {
    plugin,
    /** Access the plugin instance from a view */
    get(view: EditorView): GhostTextPlugin | null {
      const found = view.plugin(plugin)
      return (found as GhostTextPlugin | null) ?? null
    },
  }
}

// ─── Keybindings ─────────────────────────────────────

/** Must be added to editor extensions to handle Tab/Escape for ghost */
export function ghostKeymap(
  plugin: ReturnType<typeof ghostTextPlugin>,
) {
  return keymap.of([
    {
      key: "Tab",
      run: (view) => {
        const p = plugin.get(view)
        if (p?.currentGhost.text) {
          p.acceptGhost()
          return true
        }
        return false
      },
    },
    {
      key: "Escape",
      run: (view) => {
        const p = plugin.get(view)
        if (p?.currentGhost.text) {
          p.dismissGhost()
          return true
        }
        return false
      },
    },
  ])
}
