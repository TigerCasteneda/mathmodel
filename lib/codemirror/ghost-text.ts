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
  private loaded = false

  get currentGhost(): GhostState {
    return this.ghost
  }

  /** Call from outside to configure the AI fetcher */
  configure(fetcher: GhostFetcher) {
    this.fetcher = fetcher
    if (!this.loaded && this.view) {
      this.loaded = true
      this.scheduleAfterTyping()
    }
  }

  constructor(view: EditorView) {
    this.decorations = Decoration.none
    this.view = view
  }

  update(update: ViewUpdate) {
    if (update.docChanged) {
      // dispatch() not allowed during update — defer to next frame
      requestAnimationFrame(() => this.dismissGhost())
      this.debounceTimer = setTimeout(() => this.scheduleAfterTyping(), 1500)
    }
  }

  private scheduleAfterTyping() {
    if (!this.fetcher || !this.view) return
    const { state } = this.view
    const pos = state.selection.main.head
    const doc = state.doc

    // Don't suggest in the middle of text — cursor must be at end
    if (pos < doc.length) {
      // Check if cursor is at end of a line (could be mid-paragraph)
      // Allow ghost at any position for flexibility
    }

    // Cancel previous
    this.abortController?.abort()
    this.abortController = new AbortController()
    const signal = this.abortController.signal

    const prefix = doc.sliceString(Math.max(0, pos - 500), pos)
    const suffix = doc.sliceString(pos, Math.min(doc.length, pos + 200))

    this.ghost = { text: "", anchor: pos, loading: true }
    this.refreshDecorations()

    this.fetcher(
      prefix,
      suffix,
      signal,
      (token: string) => {
        if (signal.aborted) return
        this.ghost = {
          ...this.ghost,
          text: this.ghost.text + token,
          loading: false,
        }
        this.refreshDecorations()
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
