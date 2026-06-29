"use client"

import { useEffect, useState } from "react"
import { EditorView } from "@codemirror/view"

/**
 * Right-sidebar outline panel: a flat list of the current note's
 * markdown headings, indented by level. Clicking a row scrolls the
 * editor to that line and moves the caret to it.
 *
 * Performance: the editor's `updateListener` (set up by the parent)
 * calls `setHeadings()` on every doc change. We debounce by 200ms so
 * rapid keystrokes don't thrash the tree.
 */

interface Heading {
  level: number // 1..6
  text: string
  line: number // 1-based
}

const HEADING_RE = /^(#{1,6})\s+(.+?)\s*#*\s*$/gm

function extractHeadings(text: string): Heading[] {
  const out: Heading[] = []
  let match: RegExpExecArray | null
  HEADING_RE.lastIndex = 0
  while ((match = HEADING_RE.exec(text)) !== null) {
    out.push({
      level: match[1].length,
      text: match[2],
      line: text.slice(0, match.index).split("\n").length,
    })
  }
  return out
}

interface EssayOutlinePanelProps {
  editorView: EditorView | null
}

export function EssayOutlinePanel({ editorView }: EssayOutlinePanelProps) {
  const [headings, setHeadings] = useState<Heading[]>([])

  useEffect(() => {
    if (!editorView) return
    let timer: ReturnType<typeof setTimeout> | null = null
    const refresh = () => {
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        const text = editorView.state.doc.toString()
        setHeadings(extractHeadings(text))
      }, 200)
    }
    refresh()
    editorView.dom.addEventListener("focus", refresh)
    return () => {
      if (timer) clearTimeout(timer)
    }
  }, [editorView])

  if (headings.length === 0) {
    return (
      <div className="px-3 py-6 text-center text-xs text-essay-text-faint">
        No headings yet.
        <br />
        Start with <code className="text-essay-text-muted"># Title</code>.
      </div>
    )
  }

  return (
    <div className="flex flex-col py-2">
      {headings.map((h, idx) => (
        <button
          key={`${h.line}-${idx}`}
          className="essay-outline-row"
          style={{ paddingLeft: 10 + (h.level - 1) * 12 }}
          onClick={() => {
            if (!editorView) return
            const line = editorView.state.doc.line(h.line)
            editorView.dispatch({
              selection: { anchor: line.from },
              effects: EditorView.scrollIntoView(line.from, { y: "start" }),
            })
            editorView.focus()
          }}
        >
          <span className="truncate text-left">{h.text}</span>
          <span className="essay-outline-lineno">{h.line}</span>
        </button>
      ))}
    </div>
  )
}