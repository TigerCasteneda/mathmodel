"use client"

import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  FileText,
  FolderTree,
  Globe2,
  Link2,
  Loader2,
  PencilLine,
  Play,
  Save,
  Search,
  Send,
  Terminal,
  UserRound,
  Wrench,
} from "lucide-react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import remarkMath from "remark-math"
import rehypeKatex from "rehype-katex"
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter"
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import {
  aiChat,
  getAiConfigStatus,
  loadSession,
  onChatBackgroundTask,
  onChatError,
  onChatStream,
  onChatToolCall,
  setAiModel,
  type AiPermissionMode,
  type ChatBackgroundTaskEvent,
  type SessionMessage as PersistedSessionMessage,
  type ChatToolCallEvent,
} from "@/lib/tauri-api"
import { getToken, type ProjectCapability } from "@/lib/api"

// ── types ──

type Message = {
  id: string
  role: "user" | "assistant" | "system"
  content: string
  streaming?: boolean
  thinking?: { text: string; expanded: boolean }
  toolCalls?: ToolCallEntry[]
}

type ToolCallEntry = {
  id?: string
  name: string
  arguments: Record<string, unknown>
  output: string
  status: "running" | "success" | "error"
}

type BackgroundTaskEntry = {
  taskId: string
  taskType: string
  prompt: string
  status: "running" | "completed" | "error"
  result: string
}

const MODELS = [
  { value: "deepseek-v4-pro", label: "V4 Pro (Deep Reasoning)" },
  { value: "deepseek-v4-flash", label: "V4 Flash (Fast)" },
  { value: "deepseek-chat", label: "V3 Chat (General)" },
]

const PERMISSION_MODES: Array<{ value: AiPermissionMode; label: string }> = [
  { value: "default", label: "Default" },
  { value: "accept_edit", label: "Accept Edit" },
  { value: "auto", label: "Auto" },
  { value: "bypass", label: "Bypass" },
]

const SLASH_COMMANDS: Record<string, string> = {
  "/search": "Search the web",
  "/clear": "Clear conversation",
  "/save-ref": "Save a reference",
  "/model": "Switch model",
  "/explain": "Explain code/math",
  "/optimize": "Optimize code",
  "/refs": "List saved references",
}

function inferToolStatus(output: string): ToolCallEntry["status"] {
  if (output.startsWith("Error")) return "error"
  try {
    const parsed = JSON.parse(output) as { success?: boolean }
    if (parsed?.success === false) return "error"
  } catch {}
  return "success"
}

function parseToolArguments(argumentsText: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(argumentsText) as Record<string, unknown>
    return parsed && typeof parsed === "object" ? parsed : {}
  } catch {
    return {}
  }
}

function restoreMessages(sessionMessages: PersistedSessionMessage[]): Message[] {
  const restored: Message[] = []
  let activeAssistantIndex: number | null = null

  for (const message of sessionMessages) {
    if (message.role === "assistant" && (message.tool_calls?.length || 0) > 0) {
      restored.push({
        id: crypto.randomUUID(),
        role: "assistant",
        content: message.content || "",
        toolCalls: (message.tool_calls || []).map((toolCall) => ({
          id: toolCall.id,
          name: toolCall.function.name,
          arguments: parseToolArguments(toolCall.function.arguments),
          output: "",
          status: "running",
        })),
      })
      activeAssistantIndex = restored.length - 1
      continue
    }

    if (message.role === "tool") {
      if (activeAssistantIndex !== null) {
        const assistant = restored[activeAssistantIndex]
        const toolCalls = assistant?.toolCalls || []
        const index = toolCalls.findIndex((toolCall) => toolCall.id === message.tool_call_id)
        if (index >= 0) {
          const nextToolCalls = [...toolCalls]
          nextToolCalls[index] = {
            ...nextToolCalls[index],
            output: message.content || "",
            status: inferToolStatus(message.content || ""),
          }
          assistant.toolCalls = nextToolCalls
        }
      }
      continue
    }

    restored.push({
      id: crypto.randomUUID(),
      role: message.role as Message["role"],
      content: message.content || "",
    })
    activeAssistantIndex = null
  }

  return restored
}

function OrangeMark({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex items-center justify-center rounded-full border border-[#f59e0b]/70 bg-[#1f1308] text-[#f59e0b] shadow-[0_0_18px_rgba(245,158,11,0.18)]",
        className,
      )}
      aria-hidden="true"
    >
      <Play className="ml-0.5 h-[58%] w-[58%] fill-current stroke-[2.5]" />
    </span>
  )
}

// ── markdown renderer ──

function MarkdownContent({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeKatex]}
      components={{
        code({ className, children, ...props }) {
          const match = /language-(\w+)/.exec(className || "")
          const code = String(children).replace(/\n$/, "")
          if (match && code.length > 0) {
            return (
              <div className="my-2 rounded-lg border border-[#373737] overflow-hidden">
                <div className="flex items-center justify-between bg-[#1e1e1e] px-3 py-1.5 text-xs text-[#787878]">
                  <span>{match[1]}</span>
                  <button
                    className="hover:text-[#e8e8e8]"
                    onClick={() => navigator.clipboard.writeText(code)}
                  >
                    Copy
                  </button>
                </div>
                <SyntaxHighlighter
                  language={match[1]}
                  style={vscDarkPlus}
                  customStyle={{ margin: 0, borderRadius: 0, fontSize: "13px" }}
                >
                  {code}
                </SyntaxHighlighter>
              </div>
            )
          }
          return (
            <code className="rounded bg-[#232323] px-1.5 py-0.5 text-[13px] text-[#d4a574]" {...props}>
              {children}
            </code>
          )
        },
        table({ children }) {
          return <div className="my-2 overflow-x-auto"><table className="w-full border-collapse border border-[#373737] text-sm">{children}</table></div>
        },
        th({ children }) {
          return <th className="border border-[#373737] bg-[#1a1a1a] px-3 py-1.5 text-left">{children}</th>
        },
        td({ children }) {
          return <td className="border border-[#373737] px-3 py-1.5">{children}</td>
        },
        a({ href, children }) {
          return <a href={href} target="_blank" rel="noopener noreferrer" className="text-[#d4a574] underline">{children}</a>
        },
        h1({ children }) { return <h1 className="mt-4 mb-2 text-xl font-semibold">{children}</h1> },
        h2({ children }) { return <h2 className="mt-3 mb-1.5 text-lg font-semibold">{children}</h2> },
        h3({ children }) { return <h3 className="mt-2 mb-1 text-base font-semibold">{children}</h3> },
        p({ children }) { return <p className="mb-2 leading-relaxed">{children}</p> },
        ul({ children }) { return <ul className="mb-2 list-disc pl-5 space-y-1">{children}</ul> },
        ol({ children }) { return <ol className="mb-2 list-decimal pl-5 space-y-1">{children}</ol> },
        blockquote({ children }) {
          return <blockquote className="border-l-2 border-[#d4a574] bg-[#1a1a1a] px-3 py-1 my-2 italic text-[#b4b4b4]">{children}</blockquote>
        },
      }}
    >
      {content}
    </ReactMarkdown>
  )
}

// ── Claude Code-style tool call card ──

const TOOL_META: Record<string, { label: string; tone: string; icon: typeof Wrench }> = {
  tool_search: { label: "Find Tools", tone: "neutral", icon: Wrench },
  file_read: { label: "View", tone: "info", icon: FileText },
  read_file: { label: "View", tone: "info", icon: FileText },
  file_write: { label: "Write", tone: "success", icon: FileText },
  write_file: { label: "Write", tone: "success", icon: FileText },
  file_edit: { label: "Edit", tone: "success", icon: PencilLine },
  list_files: { label: "List", tone: "neutral", icon: FolderTree },
  execute_command: { label: "Bash", tone: "warning", icon: Terminal },
  search_files: { label: "Search", tone: "info", icon: Search },
  web_search: { label: "Web Search", tone: "info", icon: Globe2 },
  fetch_url: { label: "Fetch URL", tone: "info", icon: Link2 },
  start_background_task: { label: "Background", tone: "warning", icon: Loader2 },
  save_reference: { label: "Save Reference", tone: "success", icon: Save },
}

function toolToneClasses(tone: string, status: ToolCallEntry["status"]) {
  if (status === "error") return "border-l-[#f44336] bg-[#1a1111] shadow-[0_0_0_1px_rgba(244,67,54,0.18)]"
  if (status === "running") return "border-l-[#64b5f6] bg-[#121820] shadow-[0_0_28px_rgba(100,181,246,0.10)]"
  if (tone === "success") return "border-l-[#4caf50] bg-[#111811] shadow-[0_0_0_1px_rgba(76,175,80,0.12)]"
  if (tone === "warning") return "border-l-[#ff9800] bg-[#1b150d] shadow-[0_0_0_1px_rgba(255,152,0,0.12)]"
  if (tone === "info") return "border-l-[#64b5f6] bg-[#10161d] shadow-[0_0_0_1px_rgba(100,181,246,0.12)]"
  return "border-l-[#8d6e63] bg-[#171412] shadow-[0_0_0_1px_rgba(212,165,116,0.10)]"
}

function compactValue(value: unknown): string {
  if (typeof value === "string") return value.length > 120 ? `${value.slice(0, 120)}...` : value
  if (typeof value === "number" || typeof value === "boolean") return String(value)
  if (value == null) return ""
  const text = JSON.stringify(value)
  return text.length > 120 ? `${text.slice(0, 120)}...` : text
}

function primaryToolArg(args: Record<string, unknown>) {
  return compactValue(args.path ?? args.file_path ?? args.command ?? args.pattern ?? args.query ?? args.title ?? args.url ?? args.prompt ?? "")
}

function ToolCallCard({ tc }: { tc: ToolCallEntry }) {
  const [expanded, setExpanded] = useState(tc.status === "running")
  const meta = TOOL_META[tc.name] || { label: tc.name, tone: "neutral", icon: Wrench }
  const Icon = meta.icon
  const summary = primaryToolArg(tc.arguments)
  const statusLabel = tc.status === "running" ? "Running" : tc.status === "success" ? "Done" : "Error"

  return (
    <div
      className={cn(
        "cc-tool-card my-2 overflow-hidden rounded-lg border border-[#373737] border-l-2",
        toolToneClasses(meta.tone, tc.status),
        tc.status === "running" && "cc-tool-card-running",
      )}
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2 text-left"
        onClick={() => setExpanded((value) => !value)}
      >
        <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-[#373737] bg-[#232323] text-[#d4a574]">
          <Icon className="h-3.5 w-3.5" />
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-2">
            <span className="text-xs font-semibold text-[#e8e8e8]">{meta.label}</span>
            <span className="rounded-full border border-[#373737] bg-[#0d0d0d] px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-[#787878]">
              {tc.name}
            </span>
          </span>
          {summary && <span className="mt-0.5 block truncate font-mono text-[11px] text-[#b4b4b4]">{summary}</span>}
        </span>
        <span className="flex items-center gap-1.5 text-[11px] text-[#b4b4b4]">
          {tc.status === "running" ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin text-[#64b5f6]" />
          ) : tc.status === "success" ? (
            <CheckCircle2 className="h-3.5 w-3.5 text-[#4caf50]" />
          ) : (
            <AlertCircle className="h-3.5 w-3.5 text-[#f44336]" />
          )}
          {statusLabel}
        </span>
        {expanded ? <ChevronDown className="h-3.5 w-3.5 text-[#787878]" /> : <ChevronRight className="h-3.5 w-3.5 text-[#787878]" />}
      </button>

      {expanded && (
        <div className="border-t border-[#373737] bg-[#0d0d0d]/45 px-3 py-2">
          <div className="mb-2 grid gap-1.5">
            {Object.entries(tc.arguments).slice(0, 6).map(([key, value]) => (
              <div key={key} className="grid grid-cols-[88px_1fr] gap-2 text-[11px]">
                <span className="text-[#787878]">{key}</span>
                <span className="truncate font-mono text-[#b4b4b4]">{compactValue(value)}</span>
              </div>
            ))}
          </div>
          {tc.output ? (
            <pre className="max-h-52 overflow-y-auto rounded-md border border-[#2a2a2a] bg-[#111111] p-2 font-mono text-[11px] leading-5 text-[#b4b4b4]">
              {tc.output}
            </pre>
          ) : (
            <div className="flex items-center gap-2 rounded-md border border-[#2a2a2a] bg-[#111111] p-2 text-[11px] text-[#787878]">
              <span className="cc-thinking-dot" />
              Waiting for tool result
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function BackgroundTaskCard({ task }: { task: BackgroundTaskEntry }) {
  const done = task.status === "completed"
  const failed = task.status === "error"

  return (
    <div className="my-2 rounded-lg border border-[#373737] bg-[#151515] px-3 py-2 text-xs shadow-[0_0_0_1px_rgba(245,158,11,0.08)]">
      <div className="flex items-center gap-2">
        <OrangeMark className="h-6 w-6 shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="font-semibold text-[#e8e8e8]">Background {task.taskType}</span>
            <span className={cn(
              "rounded-full border px-1.5 py-0.5 text-[10px] uppercase",
              failed ? "border-[#f44336]/40 text-[#ffb4b4]" : done ? "border-[#4caf50]/40 text-[#9fd89f]" : "border-[#f59e0b]/40 text-[#f7be62]",
            )}>
              {task.status}
            </span>
          </div>
          <div className="mt-0.5 truncate font-mono text-[11px] text-[#b4b4b4]">{task.prompt}</div>
        </div>
        {failed ? (
          <AlertCircle className="h-4 w-4 text-[#f44336]" />
        ) : done ? (
          <CheckCircle2 className="h-4 w-4 text-[#4caf50]" />
        ) : (
          <Loader2 className="h-4 w-4 animate-spin text-[#f59e0b]" />
        )}
      </div>
      {task.result && (
        <pre className="mt-2 max-h-40 overflow-y-auto rounded-md border border-[#2a2a2a] bg-[#0d0d0d] p-2 font-mono text-[11px] leading-5 text-[#b4b4b4]">
          {task.result}
        </pre>
      )}
    </div>
  )
}

function ThinkingStrip() {
  return (
    <div className="cc-thinking-strip flex items-center gap-2 rounded-lg border border-[#373737] bg-[#151515] px-3 py-2 text-xs text-[#b4b4b4]">
      <span className="cc-thinking-dot" />
      <span>Modeler AI is thinking</span>
      <span className="cc-thinking-ellipsis" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
    </div>
  )
}

// ── main panel ──

export function ChatPanel({
  conversationId = "default",
  projectId,
  workspaceMode = "host",
  capabilities = [],
}: {
  conversationId?: string
  projectId?: string
  workspaceMode?: "host" | "guest"
  capabilities?: ProjectCapability[]
}) {
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState("")
  const [sending, setSending] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [selectedModel, setSelectedModel] = useState("deepseek-v4-pro")
  const [permissionMode, setPermissionMode] = useState<AiPermissionMode>("default")
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundTaskEntry[]>([])
  const [showSlashMenu, setShowSlashMenu] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)
  const seenStreamEventsRef = useRef<Set<string>>(new Set())

  useEffect(() => {
    getAiConfigStatus()
      .then((status) => setSelectedModel(status.model))
      .catch(() => {})
  }, [])

  // Load persisted session
  useEffect(() => {
    seenStreamEventsRef.current.clear()
    setLoaded(false)
    setBackgroundTasks([])
    loadSession(conversationId).then((session) => {
      const restored = restoreMessages(session.messages || [])
      setMessages(restored)
      setLoaded(true)
    }).catch(() => { setMessages([]); setLoaded(true) })
  }, [conversationId])

  // Auto-scroll
  const scrollToBottom = useCallback(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" })
  }, [])
  useEffect(() => { scrollToBottom() }, [messages, scrollToBottom])

  // Listen for streaming
  useEffect(() => {
    const offStream = onChatStream((event) => {
      if (event.conversation_id !== conversationId) return
      if (typeof event.seq === "number") {
        const key = `${event.conversation_id}:${event.seq}`
        if (seenStreamEventsRef.current.has(key)) return
        seenStreamEventsRef.current.add(key)
      }
      setMessages((prev) => {
        const next = [...prev]
        const last = next[next.length - 1]
        if (!last || last.role !== "assistant" || !last.streaming) {
          next.push({ id: crypto.randomUUID(), role: "assistant", content: event.content, streaming: !event.done })
          return next
        }
        last.content += event.content
        last.streaming = !event.done
        return next
      })
    })

    const offTool = onChatToolCall((event: ChatToolCallEvent) => {
      if (event.conversation_id !== conversationId) return
      const entry: ToolCallEntry = {
        name: event.name,
        arguments: event.arguments as Record<string, unknown>,
        output: event.output,
        status: event.status as ToolCallEntry["status"],
      }
      setMessages((prev) => {
        const next = [...prev]
        const last = next[next.length - 1]
        if (!last || last.role !== "assistant") {
          next.push({ id: crypto.randomUUID(), role: "assistant", content: "", toolCalls: [entry] })
          return next
        }
        // Merge tool calls on the last assistant message
        const existing = last.toolCalls || []
        const updated = existing.findIndex((tc) => tc.name === entry.name && tc.status === "running")
        if (updated >= 0) {
          const merged = [...existing]
          merged[updated] = entry
          last.toolCalls = merged
        } else {
          last.toolCalls = [...existing, entry]
        }
        return next
      })
    })

    const offError = onChatError((event) => {
      if (event.conversation_id !== conversationId) return
      setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: "system", content: event.message }])
    })

    const offBackground = onChatBackgroundTask((event: ChatBackgroundTaskEvent) => {
      if (event.conversation_id !== conversationId) return
      const entry: BackgroundTaskEntry = {
        taskId: event.task_id,
        taskType: event.task_type,
        prompt: event.prompt,
        status: event.status,
        result: event.result,
      }
      setBackgroundTasks((prev) => {
        const index = prev.findIndex((task) => task.taskId === entry.taskId)
        if (index < 0) return [...prev, entry]
        const next = [...prev]
        next[index] = entry
        return next
      })
    })

    return () => { offStream(); offTool(); offError(); offBackground() }
  }, [conversationId])

  const canSend = useMemo(() => input.trim().length > 0 && !sending && loaded, [input, sending, loaded])
  const waitingForAssistant = sending && messages[messages.length - 1]?.role === "user"

  const handleSubmit = async (event?: FormEvent) => {
    event?.preventDefault()
    const message = input.trim()
    if (!message || sending || !loaded) return
    setInput("")
    setSending(true)
    setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: "user", content: message }])
    try {
      await aiChat(message, conversationId, {
        workspaceMode,
        permissionMode,
        projectId,
        authToken: getToken(),
        capabilities,
      })
    } catch {
      setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: "system", content: "Chat request failed." }])
    } finally {
      setSending(false)
    }
  }

  const handleInput = (value: string) => {
    setInput(value)
    setShowSlashMenu(value === "/")
  }

  const insertSlash = (cmd: string) => {
    setInput(cmd + " ")
    setShowSlashMenu(false)
  }

  const handleModelChange = async (model: string) => {
    setSelectedModel(model)
    try {
      const status = await setAiModel(model)
      if (status) setSelectedModel(status.model)
    } catch {
      setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: "system", content: "Failed to update model." }])
    }
  }

  return (
    <section className="flex h-full min-h-0 flex-col bg-[#0d0d0d] text-[#e8e8e8]">
      {/* Header bar */}
      <div className="flex h-11 items-center gap-2 border-b border-[#373737] bg-[#121212]/95 px-3 shrink-0">
        <OrangeMark className="cc-claude-mark h-7 w-7" />
        <div className="flex flex-col leading-none">
          <span className="text-sm font-medium">Modeler AI</span>
          <span className="mt-1 text-[10px] uppercase tracking-[0.18em] text-[#787878]">Claude Code Runtime</span>
        </div>
        <span className="ml-auto flex items-center gap-1.5 rounded-full border border-[#373737] bg-[#1a1a1a] px-2 py-1 text-[11px] text-[#b4b4b4]">
          <span className={cn("h-1.5 w-1.5 rounded-full", sending ? "cc-live-dot bg-[#64b5f6]" : "bg-[#4caf50]")} />
          {sending ? "Streaming" : workspaceMode === "guest" ? "Guest Remote" : "Host Local"}
        </span>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
        {messages.length === 0 ? (
          <div className="mx-auto flex h-full max-w-2xl flex-col justify-center px-4">
            <OrangeMark className="mb-4 h-12 w-12" />
            <h2 className="text-xl font-semibold">Modeler AI</h2>
            <p className="mt-2 text-sm text-[#787878]">Mathematical modeling assistant. Search papers, write code, analyze data.</p>
            <div className="mt-4 flex flex-wrap gap-2 text-xs text-[#b4b4b4]">
              {Object.entries(SLASH_COMMANDS).slice(0, 4).map(([cmd, desc]) => (
                <button key={cmd} onClick={() => { setInput(cmd + " "); setShowSlashMenu(false) }}
                  className="rounded-md border border-[#373737] bg-[#1a1a1a] px-2 py-1 hover:border-[#d4a574] hover:text-[#e8e8e8]">
                  <span className="text-[#d4a574]">{cmd}</span> <span className="ml-1 text-[#787878]">{desc}</span>
                </button>
              ))}
            </div>
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-3 px-4 py-4">
            {messages.map((message) => (
              <div key={message.id} className={cn("flex gap-3", message.role === "user" && "justify-end")}>
                {message.role !== "user" && (
                  <OrangeMark className="mt-1 h-8 w-8 shrink-0" />
                )}
                <div className={cn(
                  "max-w-[80%] rounded-2xl px-4 py-3 text-sm transition-shadow",
                  message.role === "user" ? "bg-[#d4a574] text-[#111111]" :
                  message.role === "system" ? "border border-[#f44336]/40 bg-[#2d1b1b] text-[#ffb4b4]" :
                  "border border-[#373737] bg-[#1a1a1a] shadow-[0_0_0_1px_rgba(212,165,116,0.04)]",
                  message.role === "assistant" && message.streaming && "shadow-[0_0_32px_rgba(212,165,116,0.08)]",
                )}>
                  {/* Thinking block */}
                  {message.thinking && (
                    <div className="mb-2">
                      <button
                        onClick={() => {
                          setMessages((prev) => prev.map((m) =>
                            m.id === message.id
                              ? { ...m, thinking: { ...m.thinking!, expanded: !m.thinking!.expanded } }
                              : m
                          ))
                        }}
                        className="flex items-center gap-1 text-xs text-[#787878] hover:text-[#b4b4b4]"
                      >
                        {message.thinking.expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
                        Thinking
                      </button>
                      {message.thinking.expanded && (
                        <div className="mt-1 rounded-md border border-[#464646] bg-[#1e1e1e] p-2 text-xs italic text-[#787878]">
                          {message.thinking.text}
                        </div>
                      )}
                    </div>
                  )}

                  {/* Content */}
                  {message.role === "user" ? (
                    <div className="whitespace-pre-wrap">{message.content}</div>
                  ) : message.role === "assistant" ? (
                    <>
                      <MarkdownContent content={message.content} />
                      {message.streaming && <span className="cc-stream-cursor mt-1 inline-block" />}
                    </>
                  ) : (
                    <div>{message.content}</div>
                  )}

                  {/* Tool call cards */}
                  {message.toolCalls?.map((tc, i) => (
                    <ToolCallCard key={i} tc={tc} />
                  ))}
                </div>
                {message.role === "user" && (
                  <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[#2d2d2d] text-[#e8e8e8]">
                    <UserRound className="h-4 w-4" />
                  </div>
                )}
              </div>
            ))}
            {backgroundTasks.length > 0 && (
              <div className="ml-11 max-w-[80%]">
                {backgroundTasks.map((task) => (
                  <BackgroundTaskCard key={task.taskId} task={task} />
                ))}
              </div>
            )}
            {waitingForAssistant && (
              <div className="flex gap-3">
                <OrangeMark className="mt-1 h-8 w-8 shrink-0" />
                <div className="max-w-[80%]">
                  <ThinkingStrip />
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Input area */}
      <form onSubmit={handleSubmit} className="border-t border-[#373737] bg-[#121212] p-3 shrink-0">
        <div className="mx-auto max-w-3xl">
          {/* Model selector + slash menu */}
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <select
              value={selectedModel}
              onChange={(e) => handleModelChange(e.target.value)}
              className="rounded-md border border-[#373737] bg-[#232323] px-2 py-1 text-xs text-[#b4b4b4]"
            >
              {MODELS.map((m) => (
                <option key={m.value} value={m.value}>{m.label}</option>
              ))}
            </select>
            <div className="flex overflow-hidden rounded-md border border-[#373737] bg-[#1a1a1a]">
              {PERMISSION_MODES.map((mode) => (
                <button
                  key={mode.value}
                  type="button"
                  onClick={() => setPermissionMode(mode.value)}
                  className={cn(
                    "border-r border-[#373737] px-2 py-1 text-xs text-[#787878] last:border-r-0 hover:bg-[#232323] hover:text-[#e8e8e8]",
                    permissionMode === mode.value && "bg-[#232323] text-[#e8e8e8]",
                    permissionMode === mode.value && mode.value === "bypass" && "bg-[#3a1717] text-[#ffb4b4]",
                  )}
                >
                  {mode.label}
                </button>
              ))}
            </div>
            <span className="text-xs text-[#787878]">/ for commands</span>
          </div>
          {showSlashMenu && (
            <div className="mb-2 grid grid-cols-2 gap-1 rounded-lg border border-[#373737] bg-[#1a1a1a] p-2">
              {Object.entries(SLASH_COMMANDS).map(([cmd, desc]) => (
                <button
                  key={cmd}
                  type="button"
                  onClick={() => insertSlash(cmd)}
                  className="rounded px-2 py-1.5 text-left text-xs hover:bg-[#232323]"
                >
                  <span className="text-[#d4a574] font-medium">{cmd}</span>
                  <span className="ml-2 text-[#787878]">{desc}</span>
                </button>
              ))}
            </div>
          )}
          <div className="flex items-end gap-2 rounded-2xl border border-[#373737] bg-[#232323] p-2">
            <Textarea
              value={input}
              onChange={(e) => handleInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSubmit() }
                if (e.key === "Escape") setShowSlashMenu(false)
              }}
              placeholder="Message Modeler AI...  / for commands"
              className="min-h-10 flex-1 resize-none border-0 bg-transparent text-sm shadow-none placeholder:text-[#787878] focus-visible:ring-0"
            />
            <Button type="submit" size="icon" disabled={!canSend}
              className="h-9 w-9 rounded-full bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]">
              <Send className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </form>
    </section>
  )
}
