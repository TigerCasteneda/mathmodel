"use client"

import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react"
import { Bot, ChevronDown, ChevronRight, Send, Sparkles, UserRound } from "lucide-react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import remarkMath from "remark-math"
import rehypeKatex from "rehype-katex"
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter"
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import { aiChat, loadSession, onChatError, onChatStream, onChatToolCall, type ChatToolCallEvent } from "@/lib/tauri-api"

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
  name: string
  arguments: Record<string, unknown>
  output: string
  status: "running" | "success" | "error"
}

const MODELS = [
  { value: "deepseek-v4-pro", label: "V4 Pro (Deep Reasoning)" },
  { value: "deepseek-v4-flash", label: "V4 Flash (Fast)" },
  { value: "deepseek-chat", label: "V3 Chat (General)" },
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

// ── tool call card ──

function ToolCallCard({ tc }: { tc: ToolCallEntry }) {
  const [expanded, setExpanded] = useState(false)
  const iconMap: Record<string, string> = {
    web_search: "🔍", fetch_url: "📄", save_reference: "💾",
    read_file: "📖", write_file: "📝",
  }
  const colorMap: Record<string, string> = {
    running: "border-[#64b5f6]", success: "border-[#4caf50]", error: "border-[#f44336]",
  }
  const statusIcon: Record<string, string> = { running: "⏳", success: "✅", error: "❌" }

  return (
    <div className={cn("my-2 rounded-lg border bg-[#1a1a1a] p-3", colorMap[tc.status] || "border-[#373737]")}>
      <div
        className="flex cursor-pointer items-center gap-2 text-xs"
        onClick={() => setExpanded(!expanded)}
      >
        <span>{iconMap[tc.name] || "🔧"}</span>
        <span className="font-medium text-[#e8e8e8]">{tc.name}</span>
        <span className="ml-auto">{statusIcon[tc.status] || "✅"}</span>
        {expanded ? <ChevronDown className="h-3 w-3 text-[#787878]" /> : <ChevronRight className="h-3 w-3 text-[#787878]" />}
      </div>
      {expanded && tc.output && (
        <div className="mt-2 border-t border-[#373737] pt-2">
          <pre className="max-h-40 overflow-y-auto whitespace-pre-wrap text-xs text-[#b4b4b4]">{tc.output}</pre>
        </div>
      )}
    </div>
  )
}

// ── main panel ──

export function ChatPanel({ conversationId = "default" }: { conversationId?: string }) {
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState("")
  const [sending, setSending] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [selectedModel, setSelectedModel] = useState("deepseek-v4-pro")
  const [showSlashMenu, setShowSlashMenu] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)

  // Load persisted session
  useEffect(() => {
    setLoaded(false)
    loadSession(conversationId).then((session) => {
      const restored: Message[] = (session.messages || []).map((m) => ({
        id: crypto.randomUUID(),
        role: m.role as Message["role"],
        content: m.content,
      }))
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

    return () => { offStream(); offTool(); offError() }
  }, [conversationId])

  const canSend = useMemo(() => input.trim().length > 0 && !sending && loaded, [input, sending, loaded])

  const handleSubmit = async (event?: FormEvent) => {
    event?.preventDefault()
    const message = input.trim()
    if (!message || sending || !loaded) return
    setInput("")
    setSending(true)
    setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: "user", content: message }])
    try { await aiChat(message, conversationId) } catch {
      setMessages((prev) => [...prev, { id: crypto.randomUUID(), role: "system", content: "Chat request failed." }])
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

  return (
    <section className="flex h-full min-h-0 flex-col bg-[#0d0d0d] text-[#e8e8e8]">
      {/* Header bar */}
      <div className="flex h-10 items-center gap-2 border-b border-[#373737] px-3 shrink-0">
        <Bot className="h-4 w-4 text-[#d4a574]" />
        <span className="text-sm font-medium">Modeler AI</span>
        <span className="ml-auto text-xs text-[#787878]">Native</span>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
        {messages.length === 0 ? (
          <div className="mx-auto flex h-full max-w-2xl flex-col justify-center px-4">
            <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-[#232323] text-[#d4a574]">
              <Sparkles className="h-6 w-6" />
            </div>
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
                  <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[#232323] text-[#d4a574]">
                    <Bot className="h-4 w-4" />
                  </div>
                )}
                <div className={cn(
                  "max-w-[80%] rounded-2xl px-4 py-3 text-sm",
                  message.role === "user" ? "bg-[#d4a574] text-[#111111]" :
                  message.role === "system" ? "border border-[#f44336]/40 bg-[#2d1b1b] text-[#ffb4b4]" :
                  "bg-[#232323]",
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
                    <MarkdownContent content={message.content + (message.streaming ? "▋" : "")} />
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
          </div>
        )}
      </div>

      {/* Input area */}
      <form onSubmit={handleSubmit} className="border-t border-[#373737] bg-[#121212] p-3 shrink-0">
        <div className="mx-auto max-w-3xl">
          {/* Model selector + slash menu */}
          <div className="mb-2 flex items-center gap-2">
            <select
              value={selectedModel}
              onChange={(e) => setSelectedModel(e.target.value)}
              className="rounded-md border border-[#373737] bg-[#232323] px-2 py-1 text-xs text-[#b4b4b4]"
            >
              {MODELS.map((m) => (
                <option key={m.value} value={m.value}>{m.label}</option>
              ))}
            </select>
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
