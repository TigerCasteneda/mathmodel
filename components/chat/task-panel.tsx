"use client"

import { useEffect, useState } from "react"
import { CheckCircle2, Circle, Clock, Loader2, ListTodo } from "lucide-react"
import { cn } from "@/lib/utils"
interface TaskItem {
  id: string
  subject: string
  description: string
  status: string
  priority: string
  blocks: string[]
  blocked_by: string[]
  tags: string[]
  created_at: number
  updated_at: number
}

const STATUS_ICONS: Record<string, typeof Circle> = {
  pending: Circle,
  in_progress: Clock,
  completed: CheckCircle2,
}

const PRIORITY_COLORS: Record<string, string> = {
  low: "text-[#787878]",
  medium: "text-[#64b5f6]",
  high: "text-[#ebc396]",
  critical: "text-[#f44336]",
}

export function TaskPanel({
  conversationId,
}: {
  conversationId: string
}) {
  const [tasks, setTasks] = useState<TaskItem[]>([])
  const [loading, setLoading] = useState(false)
  const [expanded, setExpanded] = useState(true)

  useEffect(() => {
    setLoading(true)
    listTasks()
      .then(setTasks)
      .catch(() => setTasks([]))
      .finally(() => setLoading(false))
  }, [conversationId])

  if (!expanded) {
    return (
      <button
        className="flex h-8 items-center gap-1.5 border-t border-[#373737] bg-[#151515] px-3 text-xs text-[#787878] hover:text-[#e8e8e8]"
        onClick={() => setExpanded(true)}
      >
        <ListTodo className="h-3.5 w-3.5" />
        {tasks.length} tasks
      </button>
    )
  }

  return (
    <div className="border-t border-[#373737] bg-[#0d0d0d] max-h-48 overflow-y-auto">
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-[#242424] bg-[#151515]">
        <span className="flex items-center gap-1.5 text-xs font-medium text-[#b4b4b4]">
          <ListTodo className="h-3.5 w-3.5" />
          Tasks ({tasks.filter((t) => t.status !== "completed").length})
        </span>
        <button
          onClick={() => setExpanded(false)}
          className="text-xs text-[#787878] hover:text-[#e8e8e8]"
        >
          ✕
        </button>
      </div>
      {loading ? (
        <div className="flex justify-center py-4">
          <Loader2 className="h-4 w-4 animate-spin text-[#787878]" />
        </div>
      ) : tasks.length === 0 ? (
        <div className="py-4 text-center text-xs text-[#787878]">No tasks yet</div>
      ) : (
        <div className="grid gap-px">
          {tasks
            .filter((t) => t.status !== "deleted")
            .sort((a, b) => {
              const priorityOrder: Record<string, number> = { low: 0, medium: 1, high: 2, critical: 3 }
            return (priorityOrder[b.priority] || 0) - (priorityOrder[a.priority] || 0)
            })
            .map((task) => {
              const Icon = STATUS_ICONS[task.status] || Circle
              const done = task.status === "completed"
              return (
                <div
                  key={task.id}
                  className={cn(
                    "flex items-start gap-2 px-3 py-2",
                    done && "opacity-50",
                  )}
                >
                  <Icon className={cn("mt-0.5 h-3.5 w-3.5 shrink-0", done ? "text-[#4caf50]" : "text-[#787878]")} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5">
                      <span className={cn("text-xs font-medium", done && "line-through text-[#787878]")}>
                        {task.subject}
                      </span>
                      {task.priority in PRIORITY_COLORS && (
                        <span className={cn("text-[10px]", PRIORITY_COLORS[task.priority])}>
                          {task.priority}
                        </span>
                      )}
                    </div>
                    {task.blocked_by.length > 0 && (
                      <div className="mt-0.5 text-[10px] text-[#f44336]">
                        Blocked by: {task.blocked_by[0]}
                      </div>
                    )}
                  </div>
                </div>
              )
            })}
        </div>
      )}
    </div>
  )
}

async function listTasks(): Promise<TaskItem[]> {
  // Use task_store via Tauri command — placeholder until backend command exists
  // For now, return empty (task_list tool handles this via AI calls)
  return []
}
