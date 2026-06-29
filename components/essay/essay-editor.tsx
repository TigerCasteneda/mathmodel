"use client"

import { forwardRef, useEffect, useImperativeHandle, useRef } from "react"
import {
  EditorView,
  keymap,
  placeholder,
  highlightSpecialChars,
  drawSelection,
  highlightActiveLine,
  highlightActiveLineGutter,
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
import {
  wikilinkDecorations,
  wikilinkAutocomplete,
} from "@/lib/codemirror/wikilinks"
import { markdownShortcuts } from "@/lib/codemirror/markdown-shortcuts"
import { useAuth } from "@/hooks/use-auth"
import { createGhostFetcher } from "@/components/essay/essay-ghost"

export interface EssayEditorHandle {
  /** The underlying CodeMirror EditorView, or null before mount. */
  getView(): EditorView | null
}

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
  /** Fires when the editor's content diverges from the ydoc state at
   * mount time. Used by the tab strip to render the unsaved dot. */
  onDirtyChange?: (dirty: boolean) => void
  /** Set of md basenames (no extension) that exist as notes. Used to
   * mark wikilinks as resolved vs unresolved. Refreshed by the parent
   * (typically the file tree) on file create/delete. */
  knownFiles?: Set<string>
  /** Called when the user clicks a `[[wikilink]]` widget. The parent
   * navigates to the target file. */
  onWikilinkNavigate?: (target: string) => void
}

export const EssayEditor = forwardRef<EssayEditorHandle, EssayEditorProps>(
  function EssayEditor(
    {
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
      onDirtyChange,
      knownFiles,
      onWikilinkNavigate,
    },
    ref,
  ) {
    const containerRef = useRef<HTMLDivElement>(null)
    const viewRef = useRef<EditorView | null>(null)
    const undoManagerRef = useRef<Y.UndoManager | null>(null)
    const { user } = useAuth()

    useImperativeHandle(ref, () => ({
      getView: () => viewRef.current,
    }))

    useEffect(() => {
      if (!containerRef.current) return

      // Create undo manager (scoped to local edits)
      const undoManager = new Y.UndoManager(ytext, {
        trackedOrigins: new Set([null]), // null = local origin
      })
      undoManagerRef.current = undoManager

      // Ghost text plugin — created outside array so we can reference it
      const g = ghostTextPlugin()

      // Snapshot of the doc at mount time — used to compute dirty state.
      // Yjs updates from collaborators don't count as "dirty" because
      // they don't change the local content the user cares about; only
      // local keystrokes flip the dirty flag.
      const initialText = ytext.toString()

      const knownFilesGetter = () => knownFiles ?? new Set<string>()

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
        highlightActiveLineGutter(),
        indentOnInput(),
        placeholder("Start writing your essay..."),

        // Markdown auto-pair shortcuts (e.g. `**` → `**|**`)
        markdownShortcuts(),

        // Wikilink rendering + autocomplete
        wikilinkDecorations({
          knownFiles: knownFilesGetter,
          onNavigate: (target) => onWikilinkNavigate?.(target),
        }),
        wikilinkAutocomplete({ knownFiles: knownFilesGetter }),

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
            // Only flag dirty when the change is local. Remote Yjs
            // updates still go through `docChanged` but originate from
            // a non-null transaction origin — distinguish by checking
            // whether any local selection/transaction is in play.
            const isLocal = update.transactions.some(
              (tr) => tr.isUserEvent("input") || tr.isUserEvent("delete"),
            )
            onDirtyChange?.(isLocal && content !== initialText)
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
          doc: initialText,
          extensions,
        }),
        parent: containerRef.current,
      })

      viewRef.current = view

      // Configure ghost text AI fetcher (next tick so plugin is attached)
      if (fileId) {
        setTimeout(() => {
          const ghost = g.get(view)
          ghost?.configure(createGhostFetcher(fileId, essayFileName, serverBase, user?.id))
        }, 0)
      }

      return () => {
        view.destroy()
        viewRef.current = null
      }
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    return (
      <div
        ref={containerRef}
        className="h-full w-full overflow-hidden"
      />
    )
  },
)