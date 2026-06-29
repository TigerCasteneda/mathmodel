/**
 * CodeMirror extension: `[[wikilink]]` rendering + autocomplete.
 *
 * Two parts:
 *
 * 1. **Decorations** — `wikilinkDecorations(knownFiles)` scans the
 *    document for `[[name]]` / `[[name|alias]]` ranges and:
 *    - Hides the `[[` and `]]` brackets (Decoration.replace with empty content).
 *    - Wraps the inner text in a styled `<a>` widget carrying a
 *      `data-wikilink-target` attribute so the click handler can
 *      navigate. Unresolved links (target not in `knownFiles`) get
 *      a separate class for italic+grey styling.
 *    - Skips the line that contains the cursor so the user can edit
 *      the raw `[[…]]` without the live-preview hide trick fighting them.
 *
 * 2. **Autocomplete** — `wikilinkAutocomplete(knownFiles)` plugs into
 *    `@codemirror/autocomplete`. Triggers when the user types `[[`
 *    and surfaces existing filenames as completion items. Accept
 *    inserts `[[basename]]` at the cursor.
 *
 * Both extensions are pure functions returning CM extension objects;
 * wire them into your `EditorState.extensions` array alongside
 * `livePreview()`.
 */

import {
  Decoration,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type EditorView,
  type PluginValue,
  type ViewUpdate,
} from "@codemirror/view"
import {
  autocompletion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete"
import { RangeSetBuilder } from "@codemirror/state"

/** Match `[[name]]` or `[[name|alias]]`. Captures (1) the raw inner name
 * and (2) the optional alias (with the leading `|`). The inner name
 * can't contain `[`, `]`, or newline. */
const WIKILINK_RE = /\[\[([^\[\]\n|]+?)(?:\|[^\]\n]+?)?\]\]/g

/** Strip a trailing `.md` / `.markdown` so the index and lookup agree. */
function normalize(raw: string): string {
  return raw.trim().replace(/\.(md|markdown)$/i, "")
}

interface WikilinkConfig {
  /** Set of normalized basenames (without `.md`) that exist as notes
   * in the current project. Used both to drive autocomplete and to
   * mark links as resolved vs unresolved. */
  knownFiles: () => Set<string>
  /** Click handler invoked when the user clicks a wikilink widget. */
  onNavigate: (target: string, event: MouseEvent) => void
}

// ── Decorations ────────────────────────────────────────────────────────

/** Build the decoration set for the current doc. Cursor's line is left
 * alone so the user can edit `[[…]]` raw. */
function buildDecorations(view: EditorView, knownFiles: Set<string>): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>()
  const cursorLine = view.state.doc.lineAt(view.state.selection.main.head).number
  for (const { from, to } of view.visibleRanges) {
    const text = view.state.doc.sliceString(from, to)
    WIKILINK_RE.lastIndex = 0
    let match: RegExpExecArray | null
    while ((match = WIKILINK_RE.exec(text)) !== null) {
      const start = from + match.index
      const end = start + match[0].length
      const lineStart = view.state.doc.lineAt(start).number
      // Editable on the cursor's line — leave raw markdown visible.
      if (lineStart === cursorLine) continue
      const innerStart = start + 2 // after "[["
      const innerEnd = end - 2 // before "]]"
      const target = normalize(match[1])
      const resolved = knownFiles.has(target)
      const klass = resolved ? "cm-wikilink" : "cm-wikilink cm-wikilink--unresolved"

      // Hide the opening "[["
      builder.add(start, innerStart, Decoration.replace({}))
      // Style the inner text as a link via a widget — carries the
      // target so the click handler can route. We use a widget
      // (not a mark decoration) so the brackets can be hidden while
      // the text shows the alias verbatim.
      const displayText = view.state.doc.sliceString(innerStart, innerEnd)
      builder.add(
        innerStart,
        innerEnd,
        Decoration.replace({
          widget: new WikilinkWidget(target, klass, displayText),
        }),
      )
      // Hide the closing "]]"
      builder.add(innerEnd, end, Decoration.replace({}))
    }
  }
  return builder.finish()
}

class WikilinkWidget extends WidgetType {
  constructor(
    readonly target: string,
    readonly className: string,
    readonly displayText: string,
  ) {
    super()
  }
  eq(other: WikilinkWidget) {
    return (
      other.target === this.target &&
      other.className === this.className &&
      other.displayText === this.displayText
    )
  }
  toDOM() {
    const span = document.createElement("span")
    span.className = this.className
    span.setAttribute("data-wikilink-target", this.target)
    span.textContent = this.displayText
    return span
  }
  ignoreEvent() {
    return false
  }
}

function plugin(config: WikilinkConfig) {
  return ViewPlugin.fromClass(
    class implements PluginValue {
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.decorations = buildDecorations(view, config.knownFiles())
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.selectionSet || update.viewportChanged) {
          this.decorations = buildDecorations(update.view, config.knownFiles())
        }
      }
    },
    {
      decorations: (v) => v.decorations,
      eventHandlers: {
        click(e: MouseEvent, view: EditorView) {
          const target = (e.target as HTMLElement | null)?.closest(
            "[data-wikilink-target]",
          ) as HTMLElement | null
          if (!target) return false
          const name = target.dataset.wikilinkTarget
          if (!name) return false
          e.preventDefault()
          config.onNavigate(name, e)
          return true
        },
      },
    },
  )
}

/**
 * Decorations that hide `[[…]]` brackets and render the inner text
 * as a clickable link. Pass a getter that returns the current set of
 * known files (re-evaluated on each doc/selection/viewport change so
 * newly created notes are reflected immediately).
 */
export function wikilinkDecorations(config: WikilinkConfig) {
  // Wrap the plugin so we can ship a stable factory that takes
  // `config` once. CodeMirror calls `ViewPlugin.fromClass` once at
  // extension build time, so capturing config here is safe.
  return plugin(config)
}

// ── Autocomplete ───────────────────────────────────────────────────────

/**
 * Triggers on `[[` and lists existing note basenames. Accept inserts
 * `[[basename]]` at the cursor (or replaces the partial name typed so
 * far). Uses `@codemirror/autocomplete`.
 */
export function wikilinkAutocomplete(config: { knownFiles: () => Set<string> }) {
  return autocompletion({
    override: [
      (ctx: CompletionContext): CompletionResult | null => {
        // Look for `[[partial` immediately before the cursor.
        const before = ctx.matchBefore(/\[\[([^\[\]\n]*)/)
        if (!before) return null
        const partial = before.text.replace(/^\[\[/, "")
        const files = [...config.knownFiles()].sort()
        // If there's exactly one match and the user has typed it
        // fully, don't re-show — they'd just be annoyed.
        const filtered = partial
          ? files.filter((f) => f.toLowerCase().includes(partial.toLowerCase()))
          : files
        if (filtered.length === 0) return null
        return {
          from: before.from + 2, // after the `[[`
          options: filtered.slice(0, 50).map((name) => ({
            label: name,
            detail: "wikilink",
            apply: `${name}]]`,
          })),
          validFor: /^[^\]\n]*$/,
        }
      },
    ],
  })
}