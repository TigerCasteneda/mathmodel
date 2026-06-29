"use client"

import { useState } from "react"
import { useRouter } from "next/navigation"
import { ArrowLeft, PencilLine } from "lucide-react"
import { cn } from "@/lib/utils"

type SyncState = "synced" | "saving" | "offline"

interface EssayTopBarProps {
  title: string
  projectId: string
  syncState: SyncState
  collaborators: Array<{ name: string; color: string }>
  onRename: (title: string) => void
}

const syncConfig: Record<
  SyncState,
  { dot: string; label: string }
> = {
  synced: { dot: "bg-green-500", label: "Synced" },
  saving: { dot: "bg-yellow-500", label: "Saving..." },
  offline: { dot: "bg-red-500", label: "Offline" },
}

export function EssayTopBar({
  title,
  projectId,
  syncState,
  collaborators,
  onRename,
}: EssayTopBarProps) {
  const router = useRouter()
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(title)
  const config = syncConfig[syncState]

  return (
    <div className="flex h-10 items-center gap-3 border-b border-essay-border bg-essay-bg-alt px-4 shrink-0">
      {/* Back */}
      <button
        onClick={() => router.push(`/projects/${projectId}`)}
        className="flex items-center gap-1 text-xs text-essay-text-muted hover:text-essay-text transition-colors"
        title="Back to project (Ctrl+Shift+E)"
      >
        <ArrowLeft className="h-4 w-4" />
      </button>

      {/* Title */}
      {editing ? (
        <input
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            setEditing(false)
            if (draft.trim() && draft !== title) {
              onRename(draft.trim())
            } else {
              setDraft(title)
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur()
            if (e.key === "Escape") {
              setDraft(title)
              setEditing(false)
            }
          }}
          className="bg-essay-bg text-sm text-essay-text border border-essay-border-strong rounded px-2 py-0.5 outline-none focus:border-essay-text-muted min-w-[200px]"
        />
      ) : (
        <button
          onClick={() => {
            setDraft(title)
            setEditing(true)
          }}
          className="flex items-center gap-1.5 text-sm text-essay-text hover:text-white group"
        >
          <span className="font-medium">{title}</span>
          <PencilLine className="h-3 w-3 text-essay-text-faint opacity-0 group-hover:opacity-100 transition-opacity" />
        </button>
      )}

      <div className="flex-1" />

      {/* Sync indicator */}
      <div className="flex items-center gap-1.5 text-xs text-essay-text-faint">
        <span
          className={cn("inline-block h-2 w-2 rounded-full", config.dot)}
        />
        <span>{config.label}</span>
      </div>

      {/* Collaborator avatars */}
      {collaborators.length > 0 && (
        <div className="flex items-center -space-x-1.5">
          {collaborators.slice(0, 5).map((c, i) => (
            <div
              key={i}
              className="flex h-6 w-6 items-center justify-center rounded-full text-[10px] font-semibold text-white border border-essay-border"
              style={{ backgroundColor: c.color }}
              title={c.name}
            >
              {c.name
                .split(" ")
                .map((s) => s[0])
                .join("")
                .slice(0, 2)
                .toUpperCase()}
            </div>
          ))}
          {collaborators.length > 5 && (
            <div className="flex h-6 w-6 items-center justify-center rounded-full bg-essay-bg-active text-[10px] text-essay-text-muted border border-essay-border">
              +{collaborators.length - 5}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
