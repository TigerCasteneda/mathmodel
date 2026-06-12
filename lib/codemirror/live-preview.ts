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
import { RangeSetBuilder, StateField, StateEffect } from "@codemirror/state"

// ─── Callout Config ──────────────────────────────────

interface CalloutConfig {
  icon: string
  color: string
  bg: string
}

const CALLOUT_TYPES: Record<string, CalloutConfig> = {
  note: { icon: "📝", color: "#4a90d9", bg: "#151d2a" },
  warning: { icon: "⚠️", color: "#d19a66", bg: "#1f1912" },
  danger: { icon: "🔥", color: "#e06c75", bg: "#1f1315" },
  info: { icon: "ℹ️", color: "#61afef", bg: "#121e2a" },
  tip: { icon: "💡", color: "#98c379", bg: "#141d14" },
  example: { icon: "📋", color: "#c678dd", bg: "#1d1728" },
  quote: { icon: "❝", color: "#888", bg: "#181818" },
  success: { icon: "✅", color: "#98c379", bg: "#141d14" },
  failure: { icon: "❌", color: "#e06c75", bg: "#1f1315" },
  question: { icon: "❓", color: "#61afef", bg: "#121e2a" },
  abstract: { icon: "📄", color: "#4a90d9", bg: "#151d2a" },
  todo: { icon: "☐", color: "#4a90d9", bg: "#151d2a" },
}

// ─── Inline Math Widgets ─────────────────────────────

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
    try {
      const katex = require("katex")
      katex.render(this.latex, span, {
        throwOnError: false,
        displayMode: false,
        output: "html",
        strict: false,
      })
    } catch {
      span.textContent = this.latex
      span.style.color = "#d4a574"
      span.style.fontStyle = "italic"
    }
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

// ─── Image Widget ────────────────────────────────────

class ImageWidget extends WidgetType {
  constructor(
    readonly src: string,
    readonly alt: string,
    readonly filePath: string,
  ) {
    super()
  }

  eq(other: ImageWidget) {
    return other.src === this.src
  }

  toDOM() {
    const img = document.createElement("img")
    img.className = "cm-image-widget"
    img.alt = this.alt
    img.src = this.src
    img.title = this.alt || this.src
    img.loading = "lazy"

    // Click to open full image
    img.addEventListener("click", () => {
      window.open(this.src, "_blank")
    })

    // If it's a local path in Tauri, try to load as base64
    if (!this.src.startsWith("http") && this.filePath) {
      this.loadLocalImage(img)
    }

    return img
  }

  private async loadLocalImage(img: HTMLImageElement) {
    try {
      const { readFileBase64, isTauri } = await import("@/lib/tauri-api")
      if (!isTauri()) return
      const b64 = await readFileBase64(this.filePath)
      const ext = this.filePath.split(".").pop()?.toLowerCase()
      const mime =
        ext === "png"
          ? "image/png"
          : ext === "jpg" || ext === "jpeg"
            ? "image/jpeg"
            : ext === "gif"
              ? "image/gif"
              : ext === "svg"
                ? "image/svg+xml"
                : ext === "webp"
                  ? "image/webp"
                  : "image/png"
      img.src = `data:${mime};base64,${b64}`
    } catch {
      // Fallback: show alt text placeholder
      const placeholder = document.createElement("div")
      placeholder.className = "cm-image-placeholder"
      placeholder.textContent = `[Image: ${this.alt || this.filePath}]`
      placeholder.style.cssText =
        "padding:12px;color:#666;border:1px dashed #3a3a3a;border-radius:4px;font-family:sans-serif;font-size:13px;"
      img.replaceWith(placeholder)
    }
  }

  ignoreEvent() {
    return false
  }

  get estimatedHeight() {
    return 200
  }
}

// ─── Callout Widget ──────────────────────────────────

class CalloutWidget extends WidgetType {
  private config: CalloutConfig
  private title: string

  constructor(type: string, title: string) {
    super()
    this.config = CALLOUT_TYPES[type] ?? CALLOUT_TYPES.note
    this.title = title || type.charAt(0).toUpperCase() + type.slice(1)
  }

  eq(other: CalloutWidget) {
    return false // always re-render (simpler)
  }

  toDOM() {
    const container = document.createElement("div")
    container.className = "cm-callout"
    container.style.borderLeftColor = this.config.color
    container.style.backgroundColor = this.config.bg

    const header = document.createElement("div")
    header.className = "cm-callout-title"
    header.style.color = this.config.color
    header.textContent = `${this.config.icon}  ${this.title}`

    container.appendChild(header)
    return container
  }

  ignoreEvent() {
    return false
  }
}

// ─── Markdown Formatting Node Types ──────────────────

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

    // Active line (cursor position)
    const cursorLine = state.selection.main.head
      ? doc.lineAt(state.selection.main.head).number
      : -1

    // ── Pass 1: Find callout blocks at line level ──────
    this.decorateCallouts(builder, state, cursorLine)

    // ── Pass 2: Walk syntax tree for inline nodes ─────
    this.decorateSyntaxNodes(builder, state, cursorLine)

    return builder.finish()
  }

  private decorateCallouts(
    builder: RangeSetBuilder<Decoration>,
    state: EditorView["state"],
    _cursorLine: number,
  ) {
    const { doc } = state
    const lines = doc.toString().split("\n")

    let i = 0
    while (i < lines.length) {
      const line = lines[i]
      // Match callout header: > [!type] title
      const m = line.match(/^>\s*\[!(\w+)\]\s*(.*)/)
      if (!m) {
        i++
        continue
      }

      const calloutType = m[1].toLowerCase()
      const title = m[2].trim()
      if (!CALLOUT_TYPES[calloutType]) {
        i++
        continue
      }

      // Find the extent of this callout: all consecutive lines starting with ">"
      const startLine = i
      let endLine = i
      while (
        endLine + 1 < lines.length &&
        (lines[endLine + 1].startsWith(">") ||
          lines[endLine + 1].trim() === "")
      ) {
        endLine++
        // Empty line could end the block or be inside — be conservative
        if (
          lines[endLine].trim() === "" &&
          endLine + 1 < lines.length &&
          !lines[endLine + 1].startsWith(">")
        )
          break
      }

      // Also check: empty line after we already saw content means end
      for (let j = startLine + 1; j <= endLine; j++) {
        if (
          lines[j].trim() === "" &&
          j + 1 < lines.length &&
          !lines[j + 1].startsWith(">")
        ) {
          endLine = j - 1
          break
        }
      }

      const from = doc.line(startLine + 1).from
      const to = doc.line(endLine + 1).to

      // Insert callout widget at start, hide the markdown syntax
      builder.add(
        from,
        doc.line(startLine + 1).to,
        Decoration.line({
          attributes: {
            style: `border-left: 4px solid ${CALLOUT_TYPES[calloutType].color}; background-color: ${CALLOUT_TYPES[calloutType].bg};`,
          },
        }),
      )

      // Skip to after this block
      i = endLine + 1
    }
  }

  private decorateSyntaxNodes(
    builder: RangeSetBuilder<Decoration>,
    state: EditorView["state"],
    cursorLine: number,
  ) {
    const { doc } = state

    syntaxTree(state).iterate({
      enter(node) {
        const nodeName = node.name
        const from = node.from
        const to = node.to

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
            Decoration.replace({ inclusive: true }),
          )
          return
        }

        // Render inline math
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

        // Render display math
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

        // Render images inline
        if (nodeName === "Image") {
          const text = doc.sliceString(from, to)
          const urlMatch = text.match(/!\[.*\]\((.+)\)/)
          const altMatch = text.match(/!\[(.*)\]/)
          if (urlMatch) {
            const src = urlMatch[1]
            const alt = altMatch ? altMatch[1] : ""
            builder.add(
              from,
              to,
              Decoration.replace({
                widget: new ImageWidget(src, alt, src),
                inclusive: true,
              }),
            )
          }
          return
        }
      },
    })
  }

  destroy() {}
}

export function livePreview() {
  return ViewPlugin.fromClass(LivePreviewPlugin, {
    decorations: (plugin) => plugin.decorations,
  })
}
