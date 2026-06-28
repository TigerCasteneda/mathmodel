"use client"

import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Copy,
  ExternalLink,
  FileText,
  FolderTree,
  Globe2,
  Link2,
  Loader2,
  PencilLine,
  Save,
  Search,
  Send,
  SquareStop,
  Terminal,
  UserRound,
  Wrench,
  X,
} from "lucide-react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import remarkMath from "remark-math"
import rehypeKatex from "rehype-katex"
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter"
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { cn } from "@/lib/utils"
import {
  appendAssistantError,
  appendErrorTimeline,
  appendThinkingTimeline,
  applyStreamEvent,
  assistantTimelineFromContent,
  finalizeActiveAssistant,
  hasRenderableTimeline,
  updateActiveAssistant,
  upsertToolTimeline,
  upsertUsageTimeline,
  type AssistantTimelineItem,
  type Message,
  type ToolCallEntry,
} from "@/components/chat/chat-timeline"
import {
  aiChat,
  getAiConfigStatus,
  listOperations,
  loadSession,
  onAgentComplete,
  onAgentStart,
  onChatBackgroundTask,
  onChatError,
  onChatThinking,
  onChatTokenUsage,
  onPermissionRequest,
  onChatStream,
  onChatToolCall,
  resolvePermissionRequest,
  setAiModel,
  stopGeneration,
  type AiPermissionMode,
  type ChatBackgroundTaskEvent,
  type OperationEntry,
  type PermissionRequestEvent,
  type SessionMessage as PersistedSessionMessage,
  type ChatToolCallEvent,
} from "@/lib/tauri-api"
import { getToken, type ProjectCapability } from "@/lib/api"
import { useAuth } from "@/hooks/use-auth"
import { QuestionDialog } from "@/components/chat/question-dialog"
import { TaskPanel } from "@/components/chat/task-panel"
import { AgentCard } from "@/components/chat/agent-card"

// ── types ──

type BackgroundTaskEntry = {
  taskId: string
  taskType: string
  prompt: string
  status: "running" | "completed" | "error"
  result: string
}

type AgentSession = {
  session_id: string
  agent_type: string
  status: string
  prompt: string
  result?: string | null
}

type PendingPermissionRequest = PermissionRequestEvent

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
      const timeline = assistantTimelineFromContent(message.content || "")
      timeline.push(
        ...(message.tool_calls || []).map((toolCall) => ({
          id: toolCall.id,
          type: "tool" as const,
          toolCall: {
            id: toolCall.id,
            name: toolCall.function.name,
            arguments: parseToolArguments(toolCall.function.arguments),
            output: "",
            status: "running" as const,
          },
        })),
      )
      restored.push({
        id: crypto.randomUUID(),
        role: "assistant",
        content: message.content || "",
        timeline,
      })
      activeAssistantIndex = restored.length - 1
      continue
    }

    if (message.role === "tool") {
      if (activeAssistantIndex !== null) {
        const assistant = restored[activeAssistantIndex]
        assistant.timeline = (assistant.timeline || []).map((item) =>
          item.type === "tool" && item.toolCall.id === message.tool_call_id
            ? {
                ...item,
                toolCall: {
                  ...item.toolCall,
                  output: message.content || "",
                  status: inferToolStatus(message.content || ""),
                },
              }
            : item,
        )
      }
      continue
    }

    const role = message.role as Message["role"]
    restored.push({
      id: crypto.randomUUID(),
      role,
      content: message.content || "",
      timeline: role === "assistant" ? assistantTimelineFromContent(message.content || "") : undefined,
    })
    activeAssistantIndex = null
  }

  return restored
}

function OrangeMark({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "cc-claude-breathe inline-flex items-center justify-center rounded-full border border-[#d97757]/55 bg-[#1f1308] shadow-[0_0_18px_rgba(217,119,87,0.22)]",
        className,
      )}
      aria-hidden="true"
    >
      <img src="/claude-color.svg" alt="" className="h-[62%] w-[62%]" />
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

function parseToolOutput(output: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(output) as Record<string, unknown>
    return parsed
  } catch {
    return { _raw: output }
  }
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false)
  const handleCopy = () => {
    void (async () => {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setTimeout(() => setCopied(false), 1800)
    })()
  }
  return (
    <button
      type="button"
      aria-label="Copy to clipboard"
      className="inline-flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-[10px] text-[#787878] hover:bg-[#2a2a2a] hover:text-[#e8e8e8] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#d4a574]"
      onClick={(e) => { e.stopPropagation(); handleCopy() }}
    >
      <Copy className="h-3 w-3" />
      {copied ? "Copied" : ""}
    </button>
  )
}

function FetchedPageCard({ output }: { output: string }) {
  const data = parseToolOutput(output)
  const url = (data.url as string) || ""
  const content = (data.content as string) || (data._raw as string) || ""
  const provider = (data.provider as string) || undefined
  const [showContent, setShowContent] = useState(false)

  return (
    <div className="grid gap-2 text-sm">
      {/* URL row */}
      <div className="flex items-center gap-2 rounded-md border border-[#2a2a2a] bg-[#111111] px-3 py-2">
        <Globe2 className="h-4 w-4 shrink-0 text-[#d4a574]" />
        <a
          href={url}
          target="_blank"
          rel="noopener noreferrer"
          className="min-w-0 flex-1 truncate font-mono text-xs text-[#d4a574] underline underline-offset-2"
        >
          {url}
        </a>
        <div className="flex items-center gap-1 shrink-0">
          <CopyButton value={url} />
          <a
            href={url}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Open URL in new tab"
            className="inline-flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-[10px] text-[#787878] hover:bg-[#2a2a2a] hover:text-[#e8e8e8] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#d4a574]"
            onClick={(e) => e.stopPropagation()}
          >
            <ExternalLink className="h-3 w-3" />
          </a>
        </div>
      </div>

      {/* Provider + content toggle */}
      {content && (
        <>
          <button
            type="button"
            className="flex items-center gap-1.5 text-[11px] text-[#787878] hover:text-[#e8e8e8]"
            onClick={() => setShowContent((v) => !v)}
          >
            {showContent ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
            {provider ? `Content via ${provider}` : "Fetched content"}
            <span className="text-[10px] text-[#555]">({(content.length / 1024).toFixed(1)} KB)</span>
          </button>
          {showContent && (
            <div className="max-h-60 overflow-y-auto rounded-md border border-[#2a2a2a] bg-[#0d0d0d] p-3">
              <MarkdownContent content={content.slice(0, 6000)} />
              {content.length > 6000 && (
                <p className="mt-2 text-[11px] text-[#787878]">
                  Preview truncated at {((6000 / content.length) * 100).toFixed(0)}%. Open URL for full content.
                </p>
              )}
            </div>
          )}
        </>
      )}
    </div>
  )
}

function WebSearchResults({ output }: { output: string }) {
  const data = parseToolOutput(output)
  const query = (data.query as string) || ""
  const results = (data.results as Array<Record<string, unknown>>) || []

  if (!results.length) {
    return (
      <div className="py-2 text-center text-[11px] text-[#787878]">
        {query ? `No results found for "${query}".` : "No results found."}
      </div>
    )
  }

  return (
    <div className="grid gap-2">
      {query && (
        <div className="text-[11px] text-[#787878]">
          Results for <span className="font-mono text-[#b4b4b4]">{query}</span>
          <span className="ml-1 text-[10px]">({results.length} found)</span>
        </div>
      )}
      {results.map((result, index) => {
        const title = (result.title as string) || ""
        const url = (result.url as string) || ""
        const snippet = (result.snippet as string) || ""
        return (
          <div key={index} className="rounded-md border border-[#2a2a2a] bg-[#0d0d0d] px-3 py-2.5">
            {/* Title + actions */}
            <div className="flex items-start gap-2">
              <a
                href={url}
                target="_blank"
                rel="noopener noreferrer"
                className="min-w-0 flex-1 text-xs font-medium text-[#d4a574] underline underline-offset-2"
              >
                {title || url}
              </a>
              <div className="flex items-center gap-1 shrink-0">
                {url && <CopyButton value={url} />}
                {url && (
                  <a
                    href={url}
                    target="_blank"
                    rel="noopener noreferrer"
                    aria-label="Open result in new tab"
                    className="inline-flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-[10px] text-[#787878] hover:bg-[#2a2a2a] hover:text-[#e8e8e8] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#d4a574]"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <ExternalLink className="h-3 w-3" />
                  </a>
                )}
              </div>
            </div>
            {/* URL display + copy */}
            {url && (
              <div className="mt-1 flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-[#555]">{url}</span>
              </div>
            )}
            {/* Snippet */}
            {snippet && (
              <p className="mt-2 text-[11px] leading-relaxed text-[#b4b4b4] line-clamp-4">
                {snippet}
              </p>
            )}
          </div>
        )
      })}
    </div>
  )
}

function ToolStatusDot({ status }: { status: ToolCallEntry["status"] }) {
  return (
    <span
      className={cn(
        "h-2 w-2 shrink-0 rounded-full",
        status === "running" && "cc-live-dot bg-[#64b5f6]",
        status === "success" && "bg-[#4caf50]",
        status === "error" && "bg-[#f44336]",
      )}
    />
  )
}

function ToolCallCard({ tc }: { tc: ToolCallEntry }) {
  const [expanded, setExpanded] = useState(false)
  const meta = TOOL_META[tc.name] || { label: tc.name, tone: "neutral", icon: Wrench }
  const Icon = meta.icon
  const summary = primaryToolArg(tc.arguments)
  const statusLabel = tc.status === "running" ? "Running" : tc.status === "success" ? "Done" : "Error"


  return (
    <div
      className={cn(
        "cc-tool-card my-1 overflow-hidden rounded-md border border-transparent bg-[#111111]/45",
        "hover:border-[#2a2a2a] hover:bg-[#151515]",
      )}
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 px-2 py-1.5 text-left"
        onClick={() => setExpanded((value) => !value)}
      >
        <ToolStatusDot status={tc.status} />
        <Icon className="h-3.5 w-3.5 shrink-0 text-[#787878]" />
        <span className="min-w-0 flex-1">
          <span className="flex min-w-0 items-baseline gap-2">
            <span className="shrink-0 text-xs font-medium text-[#e8e8e8]">{meta.label}</span>
            <span className="min-w-0 truncate font-mono text-[11px] text-[#787878]">{tc.name}</span>
          </span>
          {summary && <span className="mt-0.5 block truncate font-mono text-[11px] text-[#b4b4b4]">{summary}</span>}
        </span>
        <span className="text-[11px] text-[#787878]">
          {statusLabel}
        </span>
        {expanded ? <ChevronDown className="h-3.5 w-3.5 text-[#787878]" /> : <ChevronRight className="h-3.5 w-3.5 text-[#787878]" />}
      </button>

      {expanded && (
        <div className="border-t border-[#242424] bg-[#0d0d0d]/55 px-3 py-2">
          {tc.name === "fetch_url" ? (
            <FetchedPageCard output={tc.output} />
          ) : tc.name === "web_search" ? (
            <WebSearchResults output={tc.output} />
          ) : (
            <>
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
            </>
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

function TimelineThinkingItem({
  item,
  active,
  onToggle,
}: {
  item: Extract<AssistantTimelineItem, { type: "thinking" }>
  active?: boolean
  onToggle: () => void
}) {
  return (
    <div className="py-1">
      <button
        type="button"
        onClick={onToggle}
        className="flex items-center gap-1.5 text-xs text-[#787878] hover:text-[#b4b4b4]"
      >
        {item.expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        <span>{active ? "Thinking..." : "Thinking"}</span>
        {active && <span className="cc-thinking-dot ml-1" />}
      </button>
      {item.expanded && (
        <div className="mt-1 whitespace-pre-wrap border-l border-[#373737] pl-3 text-xs italic leading-5 text-[#787878]">
          {item.text}
        </div>
      )}
    </div>
  )
}

function TimelineTextItem({
  item,
  active,
}: {
  item: Extract<AssistantTimelineItem, { type: "text" }>
  active?: boolean
}) {
  return (
    <div className="cc-transcript-text py-1 text-sm leading-6 text-[#e8e8e8]">
      {active ? (
        <span className="whitespace-pre-wrap">
          {item.content}
          <span className="cc-stream-cursor ml-0.5 inline align-baseline" />
        </span>
      ) : (
        <MarkdownContent content={item.content} />
      )}
    </div>
  )
}

function TimelineUsageItem({ item }: { item: Extract<AssistantTimelineItem, { type: "usage" }> }) {
  return (
    <div className="mt-1 flex items-center gap-2 text-[11px] tabular-nums text-[#787878]">
      <span className="h-px w-5 bg-[#373737]" />
      <span>
        Tokens {item.usage.prompt_tokens} prompt / {item.usage.completion_tokens} completion
      </span>
    </div>
  )
}

function TimelineStatusItem({ item }: { item: Extract<AssistantTimelineItem, { type: "stopped" | "error" }> }) {
  if (item.type === "stopped") {
    return (
      <div className="mt-1 flex items-center gap-2 text-xs text-[#787878]">
        <span className="h-1.5 w-1.5 rounded-full bg-[#787878]" />
        Generation stopped
      </div>
    )
  }

  return (
    <div className="mt-1 flex items-start gap-2 rounded-md border border-[#f44336]/30 bg-[#2d1b1b]/45 px-2 py-1.5 text-xs text-[#ffb4b4]">
      <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <span>{item.message}</span>
    </div>
  )
}

// Groups consecutive tool items into a collapsible summary.
// Auto-expands while streaming, auto-collapses when done.
function ToolsSummary({ tools, streaming }: { tools: ToolCallEntry[]; streaming?: boolean }) {
  const [expanded, setExpanded] = useState(!!streaming)
  const prevStreamingRef = useRef(streaming)

  useEffect(() => {
    if (streaming && !prevStreamingRef.current) setExpanded(true)
    else if (!streaming && prevStreamingRef.current) setExpanded(false)
    prevStreamingRef.current = streaming
  }, [streaming])

  const runningCount = tools.filter((t) => t.status === "running").length
  const errorCount = tools.filter((t) => t.status === "error").length

  return (
    <div className="my-1">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex items-center gap-1.5 text-xs text-[#787878] hover:text-[#b4b4b4]"
      >
        {expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        <Wrench className="h-3 w-3" />
        <span>
          {tools.length} tool call{tools.length !== 1 ? "s" : ""}
        </span>
        {runningCount > 0 && <span className="cc-live-dot h-1.5 w-1.5 rounded-full bg-[#64b5f6]" />}
        {errorCount > 0 && <AlertCircle className="h-3 w-3 text-[#f44336]" />}
      </button>
      {expanded && (
        <div className="mt-1 grid gap-0.5">
          {tools.map((tc) => (
            <ToolCallCard key={tc.id} tc={tc} />
          ))}
        </div>
      )}
    </div>
  )
}

type RenderSegment =
  | { type: "item"; item: AssistantTimelineItem }
  | { type: "tools"; tools: ToolCallEntry[] }

function groupTimelineSegments(timeline: AssistantTimelineItem[]): RenderSegment[] {
  const segments: RenderSegment[] = []
  let batch: ToolCallEntry[] = []

  const flush = () => {
    if (batch.length > 0) {
      segments.push({ type: "tools", tools: [...batch] })
      batch = []
    }
  }

  for (const item of timeline) {
    if (item.type === "tool") {
      batch.push(item.toolCall)
    } else {
      flush()
      segments.push({ type: "item", item })
    }
  }
  flush()
  return segments
}

function TimelineItemView({
  item,
  message,
  isLast,
  onToggleThinking,
}: {
  item: AssistantTimelineItem
  message: Message
  isLast: boolean
  onToggleThinking: (itemId: string) => void
}) {
  if (item.type === "thinking") {
    return (
      <TimelineThinkingItem
        item={item}
        active={message.streaming && isLast}
        onToggle={() => onToggleThinking(item.id)}
      />
    )
  }
  if (item.type === "text") return <TimelineTextItem item={item} active={message.streaming && isLast} />
  if (item.type === "tool") return <ToolCallCard tc={item.toolCall} />
  if (item.type === "usage") return <TimelineUsageItem item={item} />
  return <TimelineStatusItem item={item} />
}

function TranscriptTurn({
  message,
  onToggleThinking,
}: {
  message: Message
  onToggleThinking: (messageId: string, itemId: string) => void
}) {
  if (message.role === "user") {
    return (
      <div className="flex justify-end gap-3">
        <div className="max-w-[78%] rounded-lg bg-[#d4a574] px-3 py-2 text-sm leading-5 text-[#111111]">
          <div className="whitespace-pre-wrap">{message.content}</div>
        </div>
        <div className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[#2d2d2d] text-[#e8e8e8]">
          <UserRound className="h-3.5 w-3.5" />
        </div>
      </div>
    )
  }

  if (message.role === "system") {
    return (
      <div className="flex gap-3">
        <div className="mt-1 flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-[#f44336]/40 bg-[#2d1b1b] text-[#ffb4b4]">
          <AlertCircle className="h-3.5 w-3.5" />
        </div>
        <div className="max-w-[82%] rounded-md border border-[#f44336]/40 bg-[#2d1b1b] px-3 py-2 text-sm text-[#ffb4b4]">
          {message.content}
        </div>
      </div>
    )
  }

  const timeline = message.timeline || assistantTimelineFromContent(message.content)

  return (
    <div className="flex gap-3">
      <OrangeMark className="mt-0.5 h-7 w-7 shrink-0" />
      <div
        className={cn(
          "min-w-0 flex-1 border-l border-[#2a2a2a] pl-3",
          message.streaming && "border-[#d4a574]/55",
        )}
      >
        <div className="mb-1 flex items-center gap-2 text-[11px] uppercase tracking-[0.16em] text-[#787878]">
          <span>Modeler</span>
          {message.streaming && <span className="cc-live-dot h-1.5 w-1.5 rounded-full bg-[#64b5f6]" />}
        </div>
        {hasRenderableTimeline(message) ? (
          <div className="grid gap-1">
            {groupTimelineSegments(timeline).map((seg, index, arr) =>
              seg.type === "tools" ? (
                <ToolsSummary
                  key={`tools-${index}`}
                  tools={seg.tools}
                  streaming={message.streaming && index === arr.length - 1}
                />
              ) : (
                <TimelineItemView
                  key={seg.item.id}
                  item={seg.item}
                  message={message}
                  isLast={index === arr.length - 1}
                  onToggleThinking={(itemId) => onToggleThinking(message.id, itemId)}
                />
              )
            )}
          </div>
        ) : message.streaming ? (
          <ThinkingStrip />
        ) : (
          <div className="py-1 text-xs text-[#787878]">No response received.</div>
        )}
      </div>
    </div>
  )
}

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
  const { user } = useAuth()
  const sessionUserId = user?.id ?? ""
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState("")
  const [sending, setSending] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [selectedModel, setSelectedModel] = useState("deepseek-v4-pro")
  const [permissionMode, setPermissionMode] = useState<AiPermissionMode>("default")
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundTaskEntry[]>([])
  const [agentSessions, setAgentSessions] = useState<AgentSession[]>([])
  const [pendingPermissionRequests, setPendingPermissionRequests] = useState<PendingPermissionRequest[]>([])
  const [resolvingPermission, setResolvingPermission] = useState(false)
  const [stopRequested, setStopRequested] = useState(false)
  const [showSlashMenu, setShowSlashMenu] = useState(false)
  const [showOpHistory, setShowOpHistory] = useState(false)
  const [operations, setOperations] = useState<OperationEntry[]>([])
  const scrollRef = useRef<HTMLDivElement>(null)
  const seenStreamEventsRef = useRef<Set<string>>(new Set())
  const stopRequestedRef = useRef(false)

  const setStopRequestedState = useCallback((value: boolean) => {
    stopRequestedRef.current = value
    setStopRequested(value)
  }, [])

  const toggleThinkingItem = useCallback((messageId: string, itemId: string) => {
    setMessages((prev) => prev.map((message) => {
      if (message.id !== messageId || !message.timeline) return message
      return {
        ...message,
        timeline: message.timeline.map((item) =>
          item.type === "thinking" && item.id === itemId
            ? { ...item, expanded: !item.expanded }
            : item,
        ),
      }
    }))
  }, [])

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
    setPendingPermissionRequests([])
    setStopRequestedState(false)
    loadSession(sessionUserId, conversationId).then((session) => {
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
      const wasStopRequested = event.done && stopRequestedRef.current
      if (event.done) setStopRequestedState(false)
      setMessages((prev) => {
        if (typeof event.seq === "number") {
          const key = `${event.conversation_id}:${event.seq}`
          if (seenStreamEventsRef.current.has(key)) return prev
          seenStreamEventsRef.current.add(key)
        }
        return applyStreamEvent(prev, event, wasStopRequested)
      })
    })

    const offThinking = onChatThinking((event) => {
      if (event.conversation_id !== conversationId) return
      setMessages((prev) => {
        return updateActiveAssistant(prev, (message) => ({
          ...message,
          streaming: true,
          timeline: appendThinkingTimeline(message.timeline || [], event.content),
        }))
      })
    })

    const offUsage = onChatTokenUsage((event) => {
      if (event.conversation_id !== conversationId) return
      setMessages((prev) => updateActiveAssistant(prev, (message) => ({
        ...message,
        timeline: upsertUsageTimeline(message.timeline || [], event),
      })))
    })

    const offTool = onChatToolCall((event: ChatToolCallEvent) => {
      if (event.conversation_id !== conversationId) return
      const entry: ToolCallEntry = {
        id: event.id,
        name: event.name,
        arguments: event.arguments as Record<string, unknown>,
        output: event.output,
        status: event.status as ToolCallEntry["status"],
      }
      setMessages((prev) => {
        return updateActiveAssistant(prev, (message) => ({
          ...message,
          streaming: true,
          timeline: upsertToolTimeline(message.timeline || [], entry),
        }))
      })
    })

    const offError = onChatError((event) => {
      if (event.conversation_id !== conversationId) return
      setMessages((prev) => appendAssistantError(prev, event.message))
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

    const offPermission = onPermissionRequest((event: PermissionRequestEvent) => {
      if (event.conversation_id !== conversationId) return
      setPendingPermissionRequests((prev) => [...prev, event])
    })

    const offAgentStart = onAgentStart((event) => {
      setAgentSessions((prev) => [...prev, {
        session_id: event.session_id,
        agent_type: event.agent_type,
        status: event.status,
        prompt: event.prompt,
        result: event.result,
      }])
    })

    const offAgentComplete = onAgentComplete((event) => {
      setAgentSessions((prev) => prev.map((s) =>
        s.session_id === event.session_id
          ? { ...s, status: "completed", result: event.result }
          : s,
      ))
    })

    return () => { offStream(); offThinking(); offUsage(); offTool(); offError(); offBackground(); offPermission(); offAgentStart(); offAgentComplete() }
  }, [conversationId, setStopRequestedState])

  useEffect(() => {
    if (pendingPermissionRequests.length === 0) return
    const timers = pendingPermissionRequests.map((request) => {
      const remaining = Math.max(request.expires_at_ms - Date.now(), 0)
      return window.setTimeout(() => {
        setPendingPermissionRequests((prev) => prev.filter((item) => item.request_id !== request.request_id))
      }, remaining + 250)
    })

    return () => {
      for (const timer of timers) window.clearTimeout(timer)
    }
  }, [pendingPermissionRequests])

  const canSend = useMemo(() => input.trim().length > 0 && !sending && loaded, [input, sending, loaded])
  const waitingForAssistant = sending && messages[messages.length - 1]?.role === "user"

  const handleSubmit = async (event?: FormEvent) => {
    event?.preventDefault()
    const message = input.trim()
    if (!message || sending || !loaded) return
    setInput("")
    setSending(true)
    setStopRequestedState(false)
    setMessages((prev) => [
      ...prev,
      { id: crypto.randomUUID(), role: "user", content: message },
      { id: crypto.randomUUID(), role: "assistant", content: "", streaming: true, timeline: [] },
    ])
    try {
      await aiChat(message, conversationId, {
        workspaceMode,
        permissionMode,
        projectId,
        authToken: getToken(),
        capabilities,
        userId: sessionUserId,
      })
    } catch {
      setMessages((prev) => appendAssistantError(prev, "Chat request failed."))
    } finally {
      setSending(false)
      // The call has resolved, so the backend has finished streaming. Force the
      // trailing assistant message out of its streaming state in case the final
      // done:true event was dropped — otherwise its text stays in the raw
      // pre-wrap path and only renders as markdown after re-entering the panel.
      setMessages((prev) => finalizeActiveAssistant(prev))
    }
  }

  const handleStopGeneration = async () => {
    if (!sending || stopRequested) return
    setStopRequestedState(true)
    try {
      await stopGeneration(sessionUserId, conversationId)
    } catch {
      setStopRequestedState(false)
      setMessages((prev) => updateActiveAssistant(prev, (message) => ({
        ...message,
        timeline: appendErrorTimeline(message.timeline || [], "Failed to stop generation."),
      })))
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

  const activePermissionRequest = pendingPermissionRequests[0] || null

  const handlePermissionDecision = async (allow: boolean) => {
    if (!activePermissionRequest || resolvingPermission) return
    setResolvingPermission(true)
    try {
      await resolvePermissionRequest(sessionUserId, activePermissionRequest.request_id, allow)
    } catch {
      setMessages((prev) => [
        ...prev,
        { id: crypto.randomUUID(), role: "system", content: "Failed to resolve permission request." },
      ])
    } finally {
      setPendingPermissionRequests((prev) => prev.filter((item) => item.request_id !== activePermissionRequest.request_id))
      setResolvingPermission(false)
    }
  }

  return (
    <section className="flex h-full min-h-0 flex-col bg-[#0d0d0d] text-[#e8e8e8]">
      <AlertDialog open={!!activePermissionRequest}>
        <AlertDialogContent className="max-w-xl border-[#373737] bg-[#151515] text-[#e8e8e8]">
          <AlertDialogHeader>
            <AlertDialogTitle>Permission Request</AlertDialogTitle>
            <AlertDialogDescription className="text-[#b4b4b4]">
              {activePermissionRequest?.reason || "This tool execution needs approval."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {activePermissionRequest && (
            <div className="grid gap-3 text-sm">
              <div className="grid grid-cols-[96px_1fr] gap-2 rounded-md border border-[#373737] bg-[#101010] p-3">
                <span className="text-[#787878]">Tool</span>
                <span className="font-mono text-[#e8e8e8]">{activePermissionRequest.tool_name}</span>
                <span className="text-[#787878]">Mode</span>
                <span className="text-[#e8e8e8]">{PERMISSION_MODES.find((mode) => mode.value === activePermissionRequest.mode)?.label || activePermissionRequest.mode}</span>
                <span className="text-[#787878]">Content</span>
                <span className="break-all font-mono text-[#b4b4b4]">
                  {activePermissionRequest.content || compactValue(activePermissionRequest.arguments)}
                </span>
              </div>
              {pendingPermissionRequests.length > 1 && (
                <div className="text-xs text-[#787878]">
                  {pendingPermissionRequests.length - 1} more request{pendingPermissionRequests.length > 2 ? "s" : ""} queued
                </div>
              )}
            </div>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel
              className="border-[#373737] bg-transparent text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8]"
              onClick={(event) => {
                event.preventDefault()
                void handlePermissionDecision(false)
              }}
            >
              Deny
            </AlertDialogCancel>
            <AlertDialogAction
              className="bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]"
              onClick={(event) => {
                event.preventDefault()
                void handlePermissionDecision(true)
              }}
            >
              {resolvingPermission ? "Resolving..." : "Allow"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <QuestionDialog conversationId={conversationId} />

      {/* Header bar */}
      <div className="flex h-11 items-center gap-2 border-b border-[#373737] bg-[#121212]/95 px-3 shrink-0">
        <OrangeMark className="cc-claude-mark h-7 w-7" />
        <div className="flex flex-col leading-none">
          <span className="text-sm font-medium">Modeler AI</span>
          <span className="mt-1 text-[10px] uppercase tracking-[0.18em] text-[#787878]">Claude Code Runtime</span>
        </div>
        <button
          type="button"
          onClick={() => {
            setShowOpHistory((prev) => !prev)
            if (!showOpHistory) {
              listOperations(sessionUserId, conversationId).then(setOperations).catch(() => setOperations([]))
            }
          }}
          title="Operation history"
          className={cn(
            "ml-auto inline-flex items-center gap-1 rounded-full border border-[#373737] px-2 py-1 text-xs text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8] transition-colors",
            showOpHistory && "bg-[#232323] text-[#e8e8e8] border-[#d4a574]/40",
          )}
        >
          <Wrench className="h-3 w-3" />
          {operations.length}
        </button>
        <span
          role="status"
          aria-live="polite"
          className="flex items-center gap-1.5 rounded-full border border-[#373737] bg-[#1a1a1a] px-2 py-1 text-[11px] text-[#b4b4b4]"
        >
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
              <TranscriptTurn
                key={message.id}
                message={message}
                onToggleThinking={toggleThinkingItem}
              />
            ))}
            {agentSessions.length > 0 && (
              <div className="ml-11 max-w-[80%]">
                {agentSessions.map((session) => (
                  <AgentCard key={session.session_id} session={session} />
                ))}
              </div>
            )}
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

      {/* Operation history panel */}
      {showOpHistory && (
        <div className="border-t border-[#373737] bg-[#0d0d0d] px-3 py-2 max-h-48 overflow-y-auto shrink-0">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-medium text-[#b4b4b4]">Operation History</span>
            <button
              type="button"
              onClick={() => setShowOpHistory(false)}
              aria-label="Close operation history"
              className="cursor-pointer rounded text-[#787878] hover:text-[#e8e8e8] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#d4a574]"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
          {operations.length === 0 ? (
            <p className="text-xs text-[#787878]">No tool calls in this session yet.</p>
          ) : (
            <div className="grid gap-1">
              {operations.map((op) => {
                const meta = TOOL_META[op.tool_name] || { label: op.tool_name, tone: "neutral", icon: Wrench }
                const Icon = meta.icon
                return (
                  <div key={op.id} className="flex items-center gap-2 rounded px-2 py-1 text-xs hover:bg-[#1a1a1a]">
                    <span className={cn(
                      "h-1.5 w-1.5 shrink-0 rounded-full",
                      op.success ? "bg-[#4caf50]" : "bg-[#f44336]",
                    )} />
                    <Icon className="h-3 w-3 shrink-0 text-[#787878]" />
                    <span className="font-medium text-[#e8e8e8]">{meta.label}</span>
                    <span className="truncate text-[#787878]">{op.input_preview}</span>
                    <span className="ml-auto shrink-0 tabular-nums text-[#555]">
                      {new Date(op.timestamp * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
                    </span>
                  </div>
                )
              })}
            </div>
          )}
        </div>
      )}

      <TaskPanel conversationId={conversationId} />

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
            {sending ? (
              <Button
                type="button"
                size="icon"
                disabled={stopRequested}
                onClick={handleStopGeneration}
                title={stopRequested ? "Stopping" : "Stop generation"}
                className="h-9 w-9 rounded-full bg-[#f87171] text-[#111111] hover:bg-[#fca5a5]"
              >
                <SquareStop className="h-4 w-4" />
              </Button>
            ) : (
              <Button type="submit" size="icon" disabled={!canSend}
                className="h-9 w-9 rounded-full bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]">
                <Send className="h-4 w-4" />
              </Button>
            )}
          </div>
        </div>
      </form>
    </section>
  )
}
