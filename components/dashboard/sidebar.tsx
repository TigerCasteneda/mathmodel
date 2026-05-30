"use client"

import { useState } from "react"
import { 
  ChevronLeft, 
  ChevronRight,
  FileText,
  History,
  Plus,
  Sparkles
} from "lucide-react"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"

interface SidebarProps {
  collapsed: boolean
  onToggle: () => void
}

const initialChapters = [
  { id: 1, title: "Abstract" },
  { id: 2, title: "Introduction" },
  { id: 3, title: "Model Setup" },
  { id: 4, title: "Parameter Analysis" },
  { id: 5, title: "Results" },
  { id: 6, title: "Discussion" },
]

const recentChats = [
  { id: 1, title: "SIR Model Optimization", time: "2 hours ago" },
  { id: 2, title: "Heat Transfer Analysis", time: "Yesterday" },
  { id: 3, title: "Population Dynamics", time: "3 days ago" },
]

export function Sidebar({ collapsed, onToggle }: SidebarProps) {
  const [activeTab, setActiveTab] = useState<"chapters" | "history">("chapters")
  const [activeChapterId, setActiveChapterId] = useState(3)
  const [activeHistoryId, setActiveHistoryId] = useState<number | null>(null)
  const [chapters, setChapters] = useState(initialChapters)

  return (
    <div className={cn(
      "h-full bg-sidebar border-r border-sidebar-border flex flex-col transition-all duration-300",
      collapsed ? "w-16" : "w-64"
    )}>
      {/* Brand Header */}
      <div className="p-4 border-b border-sidebar-border flex items-center justify-between">
        {!collapsed && (
          <div className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg bg-primary/20 flex items-center justify-center">
              <Sparkles className="w-5 h-5 text-primary" />
            </div>
            <span className="font-semibold text-lg text-foreground">Modeler AI</span>
          </div>
        )}
        {collapsed && (
          <div className="w-8 h-8 rounded-lg bg-primary/20 flex items-center justify-center mx-auto">
            <Sparkles className="w-5 h-5 text-primary" />
          </div>
        )}
        <Button 
          variant="ghost" 
          size="icon" 
          onClick={onToggle}
          className={cn("text-muted-foreground hover:text-foreground hover:bg-sidebar-accent", collapsed && "hidden")}
        >
          <ChevronLeft className="w-4 h-4" />
        </Button>
      </div>

      {collapsed && (
        <Button 
          variant="ghost" 
          size="icon" 
          onClick={onToggle}
          className="mx-auto mt-2 text-muted-foreground hover:text-foreground hover:bg-sidebar-accent"
        >
          <ChevronRight className="w-4 h-4" />
        </Button>
      )}

      {!collapsed && (
        <>
          {/* Tab Switcher */}
          <div className="flex p-2 gap-1">
            <Button
              variant={activeTab === "chapters" ? "secondary" : "ghost"}
              size="sm"
              className="flex-1 text-xs"
              onClick={() => setActiveTab("chapters")}
            >
              <FileText className="w-3.5 h-3.5 mr-1.5" />
              Chapters
            </Button>
            <Button
              variant={activeTab === "history" ? "secondary" : "ghost"}
              size="sm"
              className="flex-1 text-xs"
              onClick={() => setActiveTab("history")}
            >
              <History className="w-3.5 h-3.5 mr-1.5" />
              History
            </Button>
          </div>

          {/* Content */}
          <ScrollArea className="flex-1 px-2">
            {activeTab === "chapters" ? (
              <div className="space-y-1 py-2">
                {chapters.map((chapter) => (
                  <button
                    key={chapter.id}
                    onClick={() => setActiveChapterId(chapter.id)}
                    className={cn(
                      "w-full text-left px-3 py-2 rounded-lg text-sm transition-all",
                      "hover:bg-sidebar-accent group",
                      chapter.id === activeChapterId 
                        ? "bg-primary/10 text-primary border border-primary/20" 
                        : "text-muted-foreground hover:text-foreground"
                    )}
                  >
                    <div className="flex items-center gap-2">
                      <span className="w-5 h-5 rounded text-xs flex items-center justify-center bg-sidebar-accent text-muted-foreground group-hover:text-foreground">
                        {chapter.id}
                      </span>
                      <span className="truncate">{chapter.title}</span>
                    </div>
                  </button>
                ))}
                <button 
                  onClick={() => {
                    const newId = chapters.length + 1
                    setChapters([...chapters, { id: newId, title: `Chapter ${newId}` }])
                    setActiveChapterId(newId)
                  }}
                  className="w-full text-left px-3 py-2 rounded-lg text-sm text-muted-foreground hover:text-foreground hover:bg-sidebar-accent transition-all flex items-center gap-2"
                >
                  <Plus className="w-4 h-4" />
                  <span>Add Chapter</span>
                </button>
              </div>
            ) : (
              <div className="space-y-1 py-2">
                {recentChats.map((chat) => (
                  <button
                    key={chat.id}
                    onClick={() => setActiveHistoryId(chat.id)}
                    className={cn(
                      "w-full text-left px-3 py-2 rounded-lg text-sm transition-all",
                      chat.id === activeHistoryId
                        ? "bg-primary/10 text-primary border border-primary/20"
                        : "text-muted-foreground hover:text-foreground hover:bg-sidebar-accent"
                    )}
                  >
                    <div className="truncate">{chat.title}</div>
                    <div className="text-xs text-muted-foreground/70">{chat.time}</div>
                  </button>
                ))}
              </div>
            )}
          </ScrollArea>

          {/* New Session Button */}
          <div className="p-3 border-t border-sidebar-border">
            <Button 
              className="w-full bg-primary/10 text-primary hover:bg-primary/20 border border-primary/20"
              size="sm"
            >
              <Plus className="w-4 h-4 mr-2" />
              New Session
            </Button>
          </div>
        </>
      )}
    </div>
  )
}
