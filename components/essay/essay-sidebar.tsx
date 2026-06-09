"use client"

import { useEffect, useState, useCallback } from "react"
import { useRouter } from "next/navigation"
import { Folder, FolderOpen, FileCode, FileText, RefreshCw } from "lucide-react"
import { cn } from "@/lib/utils"
import type { FileTreeItem } from "@/lib/tauri-api"
import * as tauriApi from "@/lib/tauri-api"

interface EssaySidebarProps {
  projectId: string
  fileId: string // current file ID, to highlight in tree
}

function fileIcon(file: FileTreeItem) {
  const ext = file.name.split(".").pop()?.toLowerCase()
  if (ext === "md") return <FileText className="h-4 w-4 text-[#d4a574]" />
  return <FileCode className="h-4 w-4 text-[#64b5f6]" />
}

export function EssaySidebar({
  projectId,
  fileId,
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

  const toggleExpand = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }, [])

  const handleFileClick = useCallback(
    (file: FileTreeItem) => {
      if (file.id === fileId) return // already open
      const ext = file.name.split(".").pop()?.toLowerCase()
      if (ext === "md") {
        // Navigate to another essay
        if (file.id) {
          router.push(
            `/projects/${projectId}/essay?file=${file.id}`,
          )
        }
      } else {
        // Navigate back to project with this file
        router.push(`/projects/${projectId}`)
      }
    },
    [projectId, fileId, router],
  )

  return (
    <div className="flex flex-col h-full bg-[#0d0d0d] border-l border-[#2a2a2a]">
      {/* Header */}
      <div className="flex items-center justify-between px-3 h-8 border-b border-[#2a2a2a] shrink-0">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-[#666]">
          Project Files
        </span>
        <button
          onClick={() =>
            tauriApi.listFiles().then(setTree).catch(() => {})
          }
          className="text-[#555] hover:text-[#888]"
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
          <div className="px-3 py-2 text-xs text-[#555]">
            Loading...
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
  onFileClick: (file: FileTreeItem) => void
}) {
  const isFolder = tree.type === "folder"
  const isExpanded = expanded.has(tree.path)
  const isActive = tree.id === activeFileId

  return (
    <div>
      <button
        className={cn(
          "flex h-7 w-full items-center gap-2 px-2 text-left text-xs text-[#b4b4b4] hover:bg-[#1a1a1a]",
          isActive && "bg-[#1e1e2e] text-[#e0e0e0]",
        )}
        style={{ paddingLeft: depth * 12 + 8 }}
        onClick={() =>
          isFolder ? onToggle(tree.path) : onFileClick(tree)
        }
      >
        {isFolder ? (
          isExpanded ? (
            <FolderOpen className="h-4 w-4 text-[#d4a574]" />
          ) : (
            <Folder className="h-4 w-4 text-[#d4a574]" />
          )
        ) : (
          fileIcon(tree)
        )}
        <span className="truncate">{tree.name}</span>
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
