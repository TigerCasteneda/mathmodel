"use client"

import { useEffect, useMemo, useState } from "react"
import { Bot, BookOpen, FileCode, FileText, Folder, FolderOpen, KeyRound, MessageSquare, RefreshCw, Settings, SidebarIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { ChatPanel } from "@/components/chat/chat-panel"
import { CodeEditor } from "@/components/editor/code-editor"
import { cn } from "@/lib/utils"
import { deleteSession, getAiConfigStatus, listSessions, setAiConfig, type AiConfigStatus, type FileTreeItem, type SessionInfo } from "@/lib/tauri-api"
import { useTauriAgent } from "@/hooks/use-tauri-agent"
import { listResearchItems, type ResearchItem } from "@/lib/api"

type Activity = "explorer" | "research" | "chat" | "settings"
type Tab = { id: string; title: string; kind: "file" | "chat" | "research"; language?: string; content?: string }

const sampleTree: FileTreeItem = {
  name: "workspace",
  path: "",
  type: "folder",
  children: [
    { name: "model.py", path: "model.py", type: "file", language: "python" },
    { name: "README.md", path: "README.md", type: "file", language: "markdown" },
  ],
}

const sampleContent = `import numpy as np

def objective(x):
    return np.sum(np.square(x))
`

const activities = [
  { id: "explorer" as const, icon: FileText, label: "Explorer" },
  { id: "research" as const, icon: BookOpen, label: "Research" },
  { id: "chat" as const, icon: MessageSquare, label: "Chat" },
  { id: "settings" as const, icon: Settings, label: "Settings" },
]

function fileLanguage(file: FileTreeItem) {
  if (file.language) return file.language
  const ext = file.name.split(".").pop()
  if (ext === "py") return "python"
  if (ext === "ts" || ext === "tsx") return "typescript"
  if (ext === "js" || ext === "jsx") return "javascript"
  if (ext === "json") return "json"
  if (ext === "md") return "markdown"
  if (ext === "tex") return "latex"
  return "plaintext"
}

function FileNode({
  item,
  depth,
  activePath,
  onOpen,
}: {
  item: FileTreeItem
  depth: number
  activePath?: string
  onOpen: (file: FileTreeItem) => void
}) {
  const [open, setOpen] = useState(depth < 1)
  const folder = item.type === "folder"

  return (
    <div>
      <button
        className={cn(
          "flex h-7 w-full items-center gap-2 px-2 text-left text-xs text-[#b4b4b4] hover:bg-[#232323]",
          activePath === item.path && "bg-[#2d2d2d] text-[#e8e8e8]",
        )}
        style={{ paddingLeft: depth * 12 + 10 }}
        onClick={() => folder ? setOpen((value) => !value) : onOpen(item)}
      >
        {folder ? (
          open ? <FolderOpen className="h-4 w-4 text-[#d4a574]" /> : <Folder className="h-4 w-4 text-[#d4a574]" />
        ) : (
          <FileCode className="h-4 w-4 text-[#64b5f6]" />
        )}
        <span className="truncate">{item.name}</span>
      </button>
      {folder && open && item.children?.map((child) => (
        <FileNode key={child.path || child.name} item={child} depth={depth + 1} activePath={activePath} onOpen={onOpen} />
      ))}
    </div>
  )
}

function ResearchLibrary({ projectId }: { projectId: string }) {
  const [items, setItems] = useState<ResearchItem[]>([])

  useEffect(() => {
    listResearchItems(projectId).then(setItems).catch(() => setItems([]))
  }, [projectId])

  return (
    <div className="h-full overflow-auto bg-[#0d0d0d] p-4 text-[#e8e8e8]">
      <div className="mx-auto max-w-3xl space-y-3">
        {items.length === 0 ? (
          <div className="py-16 text-center text-sm text-[#787878]">No references saved.</div>
        ) : items.map((item) => (
          <article key={item.id} className="rounded-md border border-[#373737] bg-[#1a1a1a] p-3">
            <div className="mb-1 text-xs uppercase text-[#d4a574]">{item.category}</div>
            <h3 className="text-sm font-medium">{item.title || "Untitled"}</h3>
            <p className="mt-2 line-clamp-3 text-xs leading-5 text-[#b4b4b4]">{item.summary}</p>
            {item.ai_relevance && (
              <p className="mt-2 text-xs leading-5 text-[#b4b4b4]">{item.ai_relevance}</p>
            )}
          </article>
        ))}
      </div>
    </div>
  )
}

const DEEPSEEK_MODELS = [
  { value: "deepseek-v4-pro", label: "V4 Pro", desc: "Deep reasoning, 32K context" },
  { value: "deepseek-v4-flash", label: "V4 Flash", desc: "Fast responses, 32K context" },
  { value: "deepseek-chat", label: "V3 Chat", desc: "General purpose, 64K context" },
]

function SettingsPanel() {
  const [status, setStatus] = useState<AiConfigStatus | null>(null)
  const [apiKey, setApiKey] = useState("")
  const [model, setModel] = useState("deepseek-v4-pro")

  useEffect(() => {
    getAiConfigStatus().then((value) => {
      setStatus(value)
      setModel(value.model)
    }).catch(() => {})
  }, [])

  const save = async () => {
    await setAiConfig({
      api_key: apiKey || null,
      base_url: "https://api.deepseek.com",
      model,
      firecrawl_api_key: null,
      searxng_url: "http://localhost:8080",
    })
    setStatus(await getAiConfigStatus())
    setApiKey("")
  }

  return (
    <div className="flex h-full items-center justify-center bg-[#0d0d0d]">
      <div className="w-full max-w-sm space-y-5 rounded-lg border border-[#373737] bg-[#1a1a1a] p-6">
        <div>
          <h3 className="text-sm font-medium text-[#e8e8e8]">Model Provider</h3>
          <p className="mt-1 text-xs text-[#787878]">DeepSeek &middot; api.deepseek.com/anthropic</p>
        </div>

        <div className="space-y-2">
          <label className="text-xs font-medium text-[#b4b4b4]">API Key</label>
          <Input
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            placeholder="sk-xxxxxxxxxxxxxxxx"
            className="border-[#373737] bg-[#232323] text-sm"
            type="password"
          />
          <p className="text-xs text-[#787878]">
            Get your key at{" "}
            <a href="https://platform.deepseek.com/api_keys" target="_blank" rel="noreferrer" className="text-[#d4a574] underline">
              platform.deepseek.com
            </a>
          </p>
        </div>

        <div className="space-y-2">
          <label className="text-xs font-medium text-[#b4b4b4]">Model</label>
          <div className="space-y-1.5">
            {DEEPSEEK_MODELS.map((m) => (
              <button
                key={m.value}
                onClick={() => setModel(m.value)}
                className={cn(
                  "w-full rounded-md border px-3 py-2 text-left",
                  model === m.value
                    ? "border-[#d4a574] bg-[#2d2d2d] text-[#e8e8e8]"
                    : "border-[#373737] bg-[#232323] text-[#b4b4b4] hover:border-[#464646]",
                )}
              >
                <div className="text-sm font-medium">{m.label}</div>
                <div className="text-xs text-[#787878]">{m.desc}</div>
              </button>
            ))}
          </div>
        </div>

        <Button
          onClick={save}
          disabled={!apiKey.trim()}
          className="w-full bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]"
        >
          {status?.configured ? "Update API Key" : "Save API Key"}
        </Button>
      </div>
    </div>
  )
}

export function ModelerWorkbench({ projectId }: { projectId: string }) {
  const tauriAgent = useTauriAgent()
  const [activeActivity, setActiveActivity] = useState<Activity>("explorer")
  const [activeTab, setActiveTab] = useState("chat")
  const [tabs, setTabs] = useState<Tab[]>([
    { id: "chat", title: "Chat", kind: "chat" },
  ])
  const [sessions, setSessions] = useState<SessionInfo[]>([])

  useEffect(() => {
    tauriAgent.connect()
  }, [tauriAgent.connect])

  const refreshSessions = async () => {
    try { setSessions(await listSessions()) } catch {}
  }

  const newChat = async () => {
    const id = crypto.randomUUID()
    const tab: Tab = { id, title: "Chat", kind: "chat" }
    setTabs((prev) => [...prev, tab])
    setActiveTab(tab.id)
    await refreshSessions()
  }

  const switchToChat = (sessionId: string, name: string) => {
    const existing = tabs.find((t) => t.id === sessionId)
    if (existing) {
      setActiveTab(sessionId)
      return
    }
    setTabs((prev) => [...prev, { id: sessionId, title: name || "Chat", kind: "chat" }])
    setActiveTab(sessionId)
  }

  const deleteChat = async (sessionId: string) => {
    try { await deleteSession(sessionId) } catch {}
    setTabs((prev) => prev.filter((t) => t.id !== sessionId))
    if (activeTab === sessionId) setActiveTab("chat")
    await refreshSessions()
  }

  const tree = tauriAgent.fileTree || sampleTree
  const active = tabs.find((tab) => tab.id === activeTab) || tabs[0]
  const activeFilePath = active?.kind === "file" ? active.id : undefined

  const openFile = async (file: FileTreeItem) => {
    const content = await tauriAgent.openFile(file.path)
    const next: Tab = {
      id: file.path,
      title: file.name,
      kind: "file",
      language: fileLanguage(file),
      content: content ?? sampleContent,
    }
    setTabs((prev) => prev.some((tab) => tab.id === next.id) ? prev.map((tab) => tab.id === next.id ? next : tab) : [...prev, next])
    setActiveTab(next.id)
  }

  const sidebarTitle = useMemo(() => {
    if (activeActivity === "explorer") return tauriAgent.workDir?.split(/[/\\]/).pop() || "Explorer"
    if (activeActivity === "research") return "Research"
    if (activeActivity === "chat") return "Chats"
    return "Settings"
  }, [activeActivity, tauriAgent.workDir])

  return (
    <main className="flex h-screen overflow-hidden bg-[#0d0d0d] text-[#e8e8e8]">
      <nav className="flex w-12 flex-col items-center border-r border-[#373737] bg-[#121212] py-2">
        {activities.map((item) => (
          <button
            key={item.id}
            title={item.label}
            onClick={() => setActiveActivity(item.id)}
            className={cn(
              "relative flex h-11 w-12 items-center justify-center text-[#787878] hover:text-[#e8e8e8]",
              activeActivity === item.id && "text-[#e8e8e8]",
            )}
          >
            <item.icon className="h-5 w-5" />
            {activeActivity === item.id && <span className="absolute left-0 top-2 h-7 w-0.5 bg-[#d4a574]" />}
          </button>
        ))}
      </nav>

      <aside className="flex w-[260px] flex-col border-r border-[#373737] bg-[#1a1a1a]">
        <div className="flex h-10 items-center gap-2 border-b border-[#373737] px-3">
          <SidebarIcon className="h-4 w-4 text-[#787878]" />
          <span className="truncate text-xs font-medium uppercase tracking-wide text-[#b4b4b4]">{sidebarTitle}</span>
          {activeActivity === "explorer" && (
            <Button size="icon" variant="ghost" className="ml-auto h-7 w-7" onClick={tauriAgent.refreshFileTree}>
              <RefreshCw className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
        <ScrollArea className="min-h-0 flex-1">
          {activeActivity === "explorer" && (
            <>
              <div className="border-b border-[#373737] px-3 py-2">
                <button
                  onClick={() => tauriAgent.openFolder()}
                  className="flex w-full items-center gap-2 rounded-md border border-[#373737] bg-[#232323] px-3 py-1.5 text-xs text-[#b4b4b4] hover:border-[#d4a574] hover:text-[#e8e8e8] transition-colors"
                >
                  <FolderOpen className="h-3.5 w-3.5 text-[#d4a574]" />
                  Open Folder
                </button>
              </div>
              <div className="py-1">
                <FileNode item={tree} depth={0} activePath={activeFilePath} onOpen={openFile} />
              </div>
            </>
          )}
          {activeActivity === "research" && (
            <button className="flex h-8 w-full items-center gap-2 px-3 text-left text-xs text-[#e8e8e8]" onClick={() => {
              if (!tabs.some((tab) => tab.id === "research")) setTabs((prev) => [...prev, { id: "research", title: "Research", kind: "research" }])
              setActiveTab("research")
            }}>
              <BookOpen className="h-4 w-4 text-[#d4a574]" />
              Library
            </button>
          )}
          {activeActivity === "chat" && (
            <>
              <button
                onClick={() => { refreshSessions(); newChat() }}
                className="flex h-8 w-full items-center gap-2 px-3 text-left text-xs text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8] transition-colors"
              >
                <span className="text-[#d4a574]">+</span>
                New Chat
              </button>
              <div className="py-1">
                {sessions.length === 0 ? (
                  <p className="px-3 py-4 text-center text-xs text-[#787878]">No conversations yet</p>
                ) : sessions.map((s) => (
                  <button
                    key={s.id}
                    onClick={() => { refreshSessions(); switchToChat(s.id, s.name || "Chat") }}
                    className="group flex h-8 w-full items-center gap-2 px-3 text-left text-xs text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8] transition-colors"
                  >
                    <MessageSquare className="h-3.5 w-3.5 text-[#d4a574]" />
                    <span className="flex-1 truncate">{s.name || new Date(s.created_at * 1000).toLocaleDateString()}</span>
                    <span className="hidden text-[#787878] group-hover:inline-flex">{s.message_count}</span>
                    <button
                      className="ml-1 hidden h-4 w-4 items-center justify-center rounded text-[#787878] hover:bg-[#373737] hover:text-[#f44336] group-hover:flex"
                      onClick={(e) => { e.stopPropagation(); deleteChat(s.id) }}
                    >
                      ✕
                    </button>
                  </button>
                ))}
              </div>
            </>
          )}
          {activeActivity === "settings" && <div className="p-3 text-xs text-[#787878]">Local configuration</div>}
        </ScrollArea>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        <div className="flex h-10 items-center overflow-x-auto border-b border-[#373737] bg-[#1a1a1a]">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                "flex h-full min-w-32 items-center gap-2 border-r border-[#373737] px-3 text-xs",
                activeTab === tab.id
                  ? "bg-[#0d0d0d] text-[#e8e8e8]"
                  : "text-[#b4b4b4] hover:bg-[#232323]",
              )}
            >
              {tab.kind === "chat" ? <MessageSquare className="h-4 w-4" /> : tab.kind === "research" ? <BookOpen className="h-4 w-4" /> : <FileCode className="h-4 w-4" />}
              <span className="truncate">{tab.title}</span>
              <span
                role="button"
                className="ml-1 flex h-4 w-4 items-center justify-center rounded hover:bg-[#373737] cursor-pointer"
                onClick={(e) => {
                  e.stopPropagation()
                  setTabs((prev) => {
                    const next = prev.filter((t) => t.id !== tab.id)
                    if (activeTab === tab.id && next.length > 0) {
                      setActiveTab(next[Math.min(prev.indexOf(tab), next.length - 1)].id)
                    }
                    return next
                  })
                }}
              >
                ✕
              </span>
            </button>
          ))}
        </div>
        <div className="min-h-0 flex-1">
          {activeActivity === "settings" ? (
            <SettingsPanel />
          ) : active?.kind === "chat" ? (
            <ChatPanel conversationId={active.id} />
          ) : active?.kind === "research" ? (
            <ResearchLibrary projectId={projectId} />
          ) : active?.kind === "file" ? (
            <CodeEditor language={active.language || "plaintext"} value={active.content || ""} />
          ) : null}
        </div>
        <footer className="flex h-7 items-center justify-between border-t border-[#373737] bg-[#1a1a1a] px-3 text-xs text-[#787878]">
          <span>{tauriAgent.status}</span>
          <span>Host: local</span>
        </footer>
      </section>
    </main>
  )
}
