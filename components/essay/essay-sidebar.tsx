"use client"

import { useEffect, useState, useCallback, type MouseEvent as ReactMouseEvent } from "react"
import { useRouter } from "next/navigation"
import {
  ChevronRight,
  FileCode,
  FileText,
  Folder,
  FolderOpen,
  RefreshCw,
} from "lucide-react"
import { cn } from "@/lib/utils"
import type { FileTreeItem } from "@/lib/tauri-api"
import * as tauriApi from "@/lib/tauri-api"

interface EssaySidebarProps {
  projectId: string
  fileId: string // current file ID, to highlight in tree
  /** Optional callback that fires with a filename when the user picks a
   * file. Lets the parent page keep the wikilink index in sync as the
   * user navigates around. */
  onKnownFilesChanged?: (basenames: Set<string>) => void
}

function fileIcon(file: FileTreeItem) {
  const ext = file.name.split(".").pop()?.toLowerCase()
  if (ext === "md") return <FileText className="essay-tree-icon" />
  return <FileCode className="essay-tree-icon" />
}

/** Strip the file extension so wikilinks and tree rows agree on the
 * canonical basename. */
function basename(file: FileTreeItem): string {
  return file.name.replace(/\.(md|markdown)$/i, "")
}

/** Walk the file tree and collect every `.md` basename. */
function collectMdBasenames(tree: FileTreeItem | null): Set<string> {
  const out = new Set<string>()
  function walk(node: FileTreeItem) {
    if (node.type === "folder") {
      node.children?.forEach(walk)
    } else if (node.type === "file") {
      const ext = node.name.split(".").pop()?.toLowerCase()
      if (ext === "md" || ext === "markdown") out.add(basename(node))
    }
  }
  if (tree) walk(tree)
  return out
}

export function EssaySidebar({
  projectId,
  fileId,
  onKnownFilesChanged,
}: EssaySidebarProps) {
  const router = useRouter()
  const [tree, setTree] = useState<FileTreeItem | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set(["/"]))

  useEffect(() => {
    // Load file tree — use Tauri or server API
    if (tauriApi.isTauri()) {
      tauriApi.listFiles().then(setTree).catch(() => setTree(null))
    }
    // Server mode tree loading handled by parent page if needed
  }, [projectId])

  // Bubble the set of known md basenames up so the editor's
  // wikilink extension can mark links as resolved vs unresolved.
  useEffect(() => {
    onKnownFilesChanged?.(collectMdBasenames(tree))
  }, [tree, onKnownFilesChanged])

  const toggleExpand = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }, [])

  const handleFileClick = useCallback(
    (file: FileTreeItem, event: ReactMouseEvent) => {
      const nextFileId = file.id ?? file.path
      if (nextFileId === fileId) return // already open
      const ext = file.name.split(".").pop()?.toLowerCase()
      if (ext === "md") {
        const params = new URLSearchParams()
        params.set("file", nextFileId)
        params.set("path", file.path)
        // Cmd/Ctrl-click opens in a new tab (preserves the current note).
        if (event.metaKey || event.ctrlKey) {
          const url = `/projects/${projectId}/essay?${params.toString()}`
          window.open(url, "_blank", "noopener,noreferrer")
          return
        }
        router.push(`/projects/${projectId}/essay?${params.toString()}`)
      } else {
        // Navigate back to project with this file
        router.push(`/projects/${projectId}`)
      }
    },
    [projectId, fileId, router],
  )

  return (
    <div className="flex flex-col h-full bg-essay-bg-sidebar border-l border-essay-border">
      {/* Header — Obsidian-style uppercase section title */}
      <div className="flex items-center justify-between essay-tree-section-title">
        <span>Files</span>
        <button
          onClick={() =>
            tauriApi.listFiles().then(setTree).catch(() => {})
          }
          className="text-essay-text-faint hover:text-essay-text-muted"
          title="Refresh file tree"
        >
          <RefreshCw className="h-3 w-3" />
        </button>
      </div>

      {/* Tree */}
      <div className="flex-1 overflow-y-auto py-1">
        {tree ? (
          <FileTreeRenderer
            tree={tree}
            depth={0}
            activeFileId={fileId}
            expanded={expanded}
            onToggle={toggleExpand}
            onFileClick={handleFileClick}
          />
        ) : (
          <div className="px-3 py-2 text-xs text-essay-text-faint">
            Loading…
          </div>
        )}
      </div>
    </div>
  )
}

// ─── Recursive Tree Renderer ──────────────────────────

function FileTreeRenderer({
  tree,
  depth,
  activeFileId,
  expanded,
  onToggle,
  onFileClick,
}: {
  tree: FileTreeItem
  depth: number
  activeFileId: string
  expanded: Set<string>
  onToggle: (path: string) => void
  onFileClick: (file: FileTreeItem, event: React.MouseEvent) => void
}) {
  const isFolder = tree.type === "folder"
  const isExpanded = expanded.has(tree.path)
  const isActive = (tree.id ?? tree.path) === activeFileId
  const indent = 8 + depth * 14

  return (
    <div className="relative">
      <button
        className={cn(
          "essay-tree-row w-full",
          isActive && "essay-tree-row--active",
        )}
        style={{ paddingLeft: indent }}
        onClick={(event) =>
          isFolder ? onToggle(tree.path) : onFileClick(tree, event)
        }
        onKeyDown={(event) => {
          if (isFolder && (event.key === "Enter" || event.key === " ")) {
            event.preventDefault()
            onToggle(tree.path)
          }
        }}
      >
        {isFolder ? (
          <ChevronRight
            className={cn(
              "essay-tree-chevron",
              isExpanded && "essay-tree-chevron--open",
            )}
          />
        ) : (
          // Spacer to align file rows with folder chevrons.
          <span className="essay-tree-chevron" aria-hidden />
        )}
        {isFolder ? (
          isExpanded ? (
            <FolderOpen className="essay-tree-icon" />
          ) : (
            <Folder className="essay-tree-icon" />
          )
        ) : (
          fileIcon(tree)
        )}
        <span className="essay-tree-name">{tree.name}</span>
      </button>
      {isFolder && isExpanded && tree.children && (
        <div>
          {tree.children.map((child) => (
            <FileTreeRenderer
              key={child.path || child.name}
              tree={child}
              depth={depth + 1}
              activeFileId={activeFileId}
              expanded={expanded}
              onToggle={onToggle}
              onFileClick={onFileClick}
            />
          ))}
        </div>
      )}
    </div>
  )
}