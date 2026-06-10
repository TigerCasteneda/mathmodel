"use client"

import { useEffect, useRef } from "react"
import {
  EditorView,
  keymap,
  placeholder,
  highlightSpecialChars,
  drawSelection,
  highlightActiveLine,
} from "@codemirror/view"
import { EditorState } from "@codemirror/state"
import { markdown, markdownLanguage } from "@codemirror/lang-markdown"
import {
  defaultKeymap,
  history,
  historyKeymap,
} from "@codemirror/commands"
import {
  syntaxHighlighting,
  defaultHighlightStyle,
  bracketMatching,
  indentOnInput,
  foldGutter,
  foldKeymap,
} from "@codemirror/language"
import { searchKeymap } from "@codemirror/search"
import * as Y from "yjs"
import { yCollab } from "y-codemirror.next"
import type { AwarenessProtocol } from "@/lib/yjs-provider"
import { essayTheme } from "@/lib/codemirror/theme"
import { livePreview } from "@/lib/codemirror/live-preview"
import { commentsPlugin } from "@/lib/codemirror/comments"
import type { EssayComment } from "@/lib/codemirror/comments"
import { ghostTextPlugin, ghostKeymap } from "@/lib/codemirror/ghost-text"
import { createGhostFetcher } from "@/components/essay/essay-ghost"

interface EssayEditorProps {
  ydoc: Y.Doc
  ytext: Y.Text
  awareness: AwarenessProtocol | null
  commentsMap?: Y.Map<EssayComment>
  /** File ID for ghost text AI conversation */
  fileId?: string
  /** Essay file name for context collection */
  essayFileName?: string
  /** Server base URL for AI calls */
  serverBase?: string
  readOnly?: boolean
  onChange?: (content: string) => void
  onCursorMove?: (line: number, col: number) => void
}

export function EssayEditor({
  ydoc,
  ytext,
  awareness,
  commentsMap,
  fileId,
  essayFileName,
  serverBase,
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

    // Ghost text plugin — created outside array so we can reference it
    const g = ghostTextPlugin()

    // Build extensions
    const extensions = [
      // Markdown language (empty codeLanguages since we don't need syntax highlighting in code blocks)
      markdown({
        base: markdownLanguage,
        codeLanguages: [],
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

      // Comments (PDF-style annotation)
      ...(commentsMap ? [commentsPlugin(commentsMap)] : []),

      // Ghost text (inline AI completion — Tabby-style)
      g.plugin,
      ghostKeymap(g),

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

    // Configure ghost text AI fetcher (next tick so plugin is attached)
    if (fileId) {
      setTimeout(() => {
        const ghost = g.get(view)
        ghost?.configure(createGhostFetcher(fileId, essayFileName, serverBase))
      }, 0)
    }

    return () => {
      view.destroy()
      viewRef.current = null
    }
  }, [])

  return (
    <div
      ref={containerRef}
      className="h-full w-full overflow-hidden"
    />
  )
}
