"use client"

import { useEffect, useState } from "react"
import { Bot, CheckCircle2, Loader2, AlertCircle, ChevronRight } from "lucide-react"
import { cn } from "@/lib/utils"

export interface AgentSession {
  session_id: string
  agent_type: string
  status: string
  prompt: string
  result?: string | null
}

export function AgentCard({ session }: { session: AgentSession }) {
  const running = session.status === "running"
  const completed = session.status === "completed"
  const failed = session.status === "failed"

  return (
    <div
      className={cn(
        "my-2 rounded-lg border px-3 py-2.5 text-xs",
        running && "border-[#f59e0b]/30 bg-[#1f1808]",
        completed && "border-[#4caf50]/30 bg-[#0d1a0d]",
        failed && "border-[#f44336]/30 bg-[#2d1b1b]",
      )}
    >
      <div className="flex items-center gap-2">
        <Bot className="h-4 w-4 shrink-0 text-[#d4a574]" />
        <span className="font-medium text-[#e8e8e8] capitalize">{session.agent_type.replace(/_/g, " ")}</span>
        <span className="ml-auto">
          {running ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin text-[#f59e0b]" />
          ) : completed ? (
            <CheckCircle2 className="h-3.5 w-3.5 text-[#4caf50]" />
          ) : (
            <AlertCircle className="h-3.5 w-3.5 text-[#f44336]" />
          )}
        </span>
      </div>
      <div className="mt-1 truncate text-[#787878]">{session.prompt}</div>
      {session.result && (
        <div className="mt-1.5 max-h-24 overflow-y-auto rounded bg-[#0d0d0d] p-2 text-[11px] leading-relaxed text-[#b4b4b4]">
          {session.result.slice(0, 500)}
          {session.result.length > 500 && "..."}
        </div>
      )}
    </div>
  )
}
