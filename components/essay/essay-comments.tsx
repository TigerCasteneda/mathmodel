"use client"

import { useEffect, useState, useCallback } from "react"
import { MessageSquare, Trash2, X } from "lucide-react"
import type { EssayComment } from "@/lib/codemirror/comments"
import { getCommentsArray, deleteComment } from "@/lib/codemirror/comments"
import * as Y from "yjs"

interface EssayCommentsPanelProps {
  commentsMap: Y.Map<EssayComment>
  onScrollTo: (comment: EssayComment) => void
}

export function EssayCommentsPanel({
  commentsMap,
  onScrollTo,
}: EssayCommentsPanelProps) {
  const [comments, setComments] = useState<EssayComment[]>([])

  useEffect(() => {
    const update = () => setComments(getCommentsArray(commentsMap))
    update()
    commentsMap.observe(update)
    return () => commentsMap.unobserve(update)
  }, [commentsMap])

  const handleDelete = useCallback(
    (id: string) => {
      deleteComment(commentsMap, id)
    },
    [commentsMap],
  )

  return (
    <div className="flex flex-col h-full bg-[#0d0d0d] border-l border-[#2a2a2a]">
      {/* Header */}
      <div className="flex items-center px-3 h-8 border-b border-[#2a2a2a] shrink-0">
        <MessageSquare className="h-3.5 w-3.5 text-[#888] mr-2" />
        <span className="text-[10px] font-semibold uppercase tracking-wider text-[#666]">
          Comments
        </span>
        {comments.length > 0 && (
          <span className="ml-2 text-[10px] text-[#555] bg-[#222] rounded-full px-1.5">
            {comments.length}
          </span>
        )}
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto py-1">
        {comments.length === 0 ? (
          <div className="px-3 py-4 text-xs text-[#555] text-center">
            Select text to add a comment
          </div>
        ) : (
          comments.map((c) => (
            <div
              key={c.id}
              className="group/comment mx-2 mb-1 rounded border border-[#2a2a2a] bg-[#111] hover:border-[#3a3a3a] cursor-pointer"
              onClick={() => onScrollTo(c)}
            >
              <div className="flex items-center gap-2 px-2 py-1.5">
                {/* Color dot */}
                <span
                  className="w-2 h-2 rounded-full shrink-0"
                  style={{ backgroundColor: c.color }}
                />
                {/* Author */}
                <span className="text-[11px] text-[#888] truncate flex-1">
                  {c.author}
                </span>
                {/* Delete */}
                <button
                  className="opacity-0 group-hover/comment:opacity-100 text-[#555] hover:text-[#e06c75] shrink-0"
                  onClick={(e) => {
                    e.stopPropagation()
                    handleDelete(c.id)
                  }}
                  title="Delete comment"
                >
                  <Trash2 className="h-3 w-3" />
                </button>
              </div>
              {/* Text */}
              <div className="px-2 pb-2 text-xs text-[#ccc] leading-relaxed whitespace-pre-wrap break-words line-clamp-3">
                {c.text}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  )
}
