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
    span.textContent = this.latex
    span.style.color = "#d4a574"
    span.style.fontStyle = "italic"
    return span
  }

  ignoreEvent() {
    return false
  }
}

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
      // eslint-disable-next-line @typescript-eslint/no-require-imports
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
    if (update.docChanged || update.selectionSet || update.viewportChanged) {
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
        if (cursorLine >= nodeStartLine && cursorLine <= nodeEndLine) {
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

        // Render inline math with widget
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

        // Render display math blocks
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
