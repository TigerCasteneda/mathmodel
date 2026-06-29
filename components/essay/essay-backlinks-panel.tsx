"use client"

import { useEffect, useState } from "react"
import { useRouter } from "next/navigation"
import { ArrowUpRight } from "lucide-react"
import type { WikilinkIndex } from "@/lib/wikilink-index"

/**
 * Right-sidebar Backlinks panel: lists every other note in the project
 * that mentions the current note via `[[current-basename]]`.
 *
 * The index lives in `lib/wikilink-index.ts` (one instance per
 * project, persisted to localStorage). The parent page passes both
 * the index and the current file's basename; this component just
 * reads `index.backlinks(basename)` and renders the result.
 *
 * When the user clicks a backlink, the parent navigates with the
 * standard essay query params so the existing file-load flow
 * (Tauri or server) handles the rest.
 */

interface EssayBacklinksPanelProps {
  projectId: string
  currentBasename: string
  index: WikilinkIndex | null
}

export function EssayBacklinksPanel({
  projectId,
  currentBasename,
  index,
}: EssayBacklinksPanelProps) {
  const router = useRouter()
  const [sources, setSources] = useState<string[]>([])

  // Re-query the index whenever the current note or the index
  // instance changes. The index mutates in place (no React state
  // churn), so we re-read on a tick after the parent signals a
  // debounced update by re-rendering with the same index ref.
  useEffect(() => {
    if (!index) {
      setSources([])
      return
    }
    setSources(index.backlinks(currentBasename))
  }, [index, currentBasename])

  if (sources.length === 0) {
    return (
      <div className="px-3 py-6 text-center text-xs text-essay-text-faint">
        No backlinks yet.
        <br />
        Link to this note from another note using{" "}
        <code className="text-essay-text-muted">[[{currentBasename}]]</code>.
      </div>
    )
  }

  return (
    <div className="flex flex-col py-1">
      {sources.map((source) => (
        <button
          key={source}
          className="essay-tree-row"
          onClick={() => {
            const params = new URLSearchParams()
            params.set("file", `${source}.md`)
            params.set("path", `${source}.md`)
            router.push(`/projects/${projectId}/essay?${params.toString()}`)
          }}
          title={`Open ${source}`}
        >
          <FileTextGlyph />
          <span className="essay-tree-name">{source}</span>
          <ArrowUpRight className="essay-tree-icon" />
        </button>
      ))}
    </div>
  )
}

function FileTextGlyph() {
  // Tiny inline glyph — keeps the row visually consistent with the
  // file tree rows in EssaySidebar without pulling the lucide icon
  // into a new import line per call site.
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="essay-tree-icon shrink-0 text-essay-text-faint"
      aria-hidden
    >
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" />
      <line x1="9" y1="13" x2="15" y2="13" />
      <line x1="9" y1="17" x2="13" y2="17" />
    </svg>
  )
}