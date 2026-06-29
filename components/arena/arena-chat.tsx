"use client"

import {
  FormEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react"
import {
  AlertCircle,
  ArrowDown,
  FileIcon,
  ImageIcon,
  Loader2,
  Paperclip,
  Reply,
  Send,
  UserRound,
  X,
} from "lucide-react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import remarkMath from "remark-math"
import rehypeKatex from "rehype-katex"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import {
  fetchChatHistory,
  uploadChatFile,
  type ChatMessage,
  type ChatHistoryPage,
  type OnlineUser,
  type ProjectCapability,
} from "@/lib/api"
import {
  ArenaChatWebSocket,
  type ChatWsEvent,
} from "@/lib/arena-chat-ws"
import { useAuth } from "@/hooks/use-auth"

// ── helpers ──

function createTempId(): string {
  return `temp_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
}

function formatTime(ts: number): string {
  const d = new Date(ts)
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
}

function formatDate(d: Date): string {
  const now = new Date()
  const today = now.toDateString()
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1).toDateString()
  const dateStr = d.toDateString()

  if (dateStr === today) return "Today"
  if (dateStr === yesterday) return "Yesterday"
  return d.toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" })
}

function groupMessagesByDate(
  messages: ChatMessage[],
): Array<{ date: Date; messages: ChatMessage[] }> {
  const groups: Array<{ date: Date; messages: ChatMessage[] }> = []
  for (const msg of messages) {
    const msgDate = new Date(msg.created_at)
    const last = groups[groups.length - 1]
    if (last && last.date.toDateString() === msgDate.toDateString()) {
      last.messages.push(msg)
    } else {
      groups.push({ date: msgDate, messages: [msg] })
    }
  }
  return groups
}

function fileIconForMime(mime: string | null | undefined) {
  if (!mime) return <FileIcon className="h-4 w-4" />
  if (mime.startsWith("image/")) return <ImageIcon className="h-4 w-4" />
  return <FileIcon className="h-4 w-4" />
}

// ── sub-components ──

function AvatarBubble({ name, userId }: { name: string; userId: string }) {
  const initial = (name || userId || "?").charAt(0).toUpperCase()
  // deterministic color from user_id
  const hue = userId
    .split("")
    .reduce((acc, ch) => acc + ch.charCodeAt(0), 0) % 360
  return (
    <span
      className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-xs font-bold text-white"
      style={{ backgroundColor: `hsl(${hue}, 55%, 45%)` }}
      title={name}
    >
      {initial}
    </span>
  )
}

function MarkdownContent({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeKatex]}
      components={{
        p({ children }) {
          return <p className="mb-1 last:mb-0 leading-relaxed">{children}</p>
        },
        code({ className, children, ...props }) {
          const isBlock = /language-(\w+)/.exec(className || "")
          if (isBlock) {
            return (
              <pre className="my-2 overflow-x-auto rounded-md bg-[#0d0d0d] p-3 text-xs">
                <code className={className} {...props}>{children}</code>
              </pre>
            )
          }
          return (
            <code className="rounded bg-[#232323] px-1 py-0.5 text-xs text-[#d4a574]" {...props}>
              {children}
            </code>
          )
        },
        a({ href, children }) {
          return <a href={href} target="_blank" rel="noopener noreferrer" className="text-[#d4a574] underline">{children}</a>
        },
      }}
    >
      {content}
    </ReactMarkdown>
  )
}

function SystemMessage({ content }: { content: string }) {
  return (
    <div className="flex justify-center py-2">
      <span className="rounded-full bg-[#1f1f1f] px-3 py-0.5 text-[11px] text-[#787878]">
        {content}
      </span>
    </div>
  )
}

function ReplyChip({
  repliedTo,
  onCancel,
}: {
  repliedTo: Pick<ChatMessage, "display_name" | "content"> | { user_id?: string; display_name: string; content_preview: string }
  onCancel?: () => void
}) {
  const name = repliedTo.display_name || "User"
  const content = "content_preview" in repliedTo
    ? repliedTo.content_preview
    : (repliedTo.content || "").slice(0, 120)
  return (
    <div className="mb-1 flex items-center gap-1.5 rounded-md border border-[#2a2a2a] bg-[#1a1a1a] px-2 py-1 text-xs">
      <Reply className="h-3 w-3 text-[#787878]" />
      <span className="text-[#d4a574]">{name}</span>
      <span className="max-w-[200px] truncate text-[#787878]">{content}</span>
      {onCancel && (
        <button onClick={onCancel} className="ml-auto text-[#787878] hover:text-[#e8e8e8]">
          <X className="h-3 w-3" />
        </button>
      )}
    </div>
  )
}

function FileCard({
  fileId,
  fileName,
  fileMime,
  projectId,
}: {
  fileId: string
  fileName?: string | null
  fileMime?: string | null
  projectId: string
}) {
  const isImage = fileMime?.startsWith("image/")
  const downloadUrl = `${process.env.NEXT_PUBLIC_API_URL ?? ""}/projects/${projectId}/files/${fileId}/download`

  if (isImage) {
    return (
      <div className="my-1 overflow-hidden rounded-md border border-[#2a2a2a]">
        <img
          src={downloadUrl}
          alt={fileName || "image"}
          className="max-h-60 w-full object-contain bg-[#0d0d0d]"
          loading="lazy"
        />
        {fileName && (
          <div className="border-t border-[#2a2a2a] bg-[#111] px-2 py-1 text-xs text-[#b4b4b4]">
            {fileName}
          </div>
        )}
      </div>
    )
  }

  return (
    <a
      href={downloadUrl}
      target="_blank"
      rel="noopener noreferrer"
      className="my-1 flex items-center gap-2 rounded-md border border-[#2a2a2a] bg-[#151515] px-3 py-2 text-xs hover:border-[#d4a574]"
    >
      {fileIconForMime(fileMime)}
      <span className="min-w-0 flex-1 truncate text-[#b4b4b4]">{fileName || fileId}</span>
    </a>
  )
}

function ChatBubble({
  message,
  isMine,
  projectId,
  onReply,
}: {
  message: ChatMessage
  isMine: boolean
  projectId: string
  onReply: (msg: ChatMessage) => void
}) {
  if (message.content_type === "system") {
    return <SystemMessage content={message.content} />
  }

  const failed = message.status === "failed"
  const sending = message.status === "sending"

  return (
    <div className={cn("flex gap-2 py-1.5", isMine && "flex-row-reverse")}>
      <AvatarBubble name={message.display_name} userId={message.user_id} />
      <div className={cn("min-w-0 max-w-[75%]", isMine && "items-end")}>
        {/* header */}
        <div className={cn("mb-0.5 flex items-center gap-2 text-xs", isMine && "flex-row-reverse")}>
          <span className="font-medium text-[#e8e8e8]">{message.display_name}</span>
          <span className="text-[#787878]">{formatTime(message.created_at)}</span>
          {failed && <AlertCircle className="h-3 w-3 text-[#f44336]" />}
        </div>

        {/* reply reference */}
        {message.replied_to && (
          <ReplyChip
            repliedTo={{
              display_name: message.replied_to.display_name,
              content_preview: message.replied_to.content_preview,
            }}
          />
        )}

        {/* message bubble */}
        <div
          className={cn(
            "rounded-lg px-3 py-2 text-sm leading-relaxed",
            isMine
              ? "bg-[#d4a574] text-[#111111]"
              : "bg-[#1f1f1f] text-[#e8e8e8]",
            sending && "opacity-60",
            failed && "border border-[#f44336]/40",
          )}
        >
          {message.content && <MarkdownContent content={message.content} />}
          {message.file_id && (
            <FileCard
              fileId={message.file_id}
              fileName={message.file_name}
              fileMime={message.file_mime}
              projectId={projectId}
            />
          )}
        </div>

        {/* actions */}
        <div className={cn("mt-0.5 flex gap-1", isMine && "flex-row-reverse")}>
          {!sending && message.content_type !== "system" && (
            <button
              onClick={() => onReply(message)}
              className="rounded px-1 py-0.5 text-[11px] text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8]"
            >
              Reply
            </button>
          )}
          {failed && (
            <button
              onClick={() => onReply(message)}
              className="rounded px-1 py-0.5 text-[11px] text-[#f44336] hover:bg-[#2d1b1b]"
            >
              Retry
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

// ── main component ──

export function ArenaChat({
  projectId,
  capabilities = [],
}: {
  projectId: string
  capabilities?: ProjectCapability[]
}) {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [onlineUsers, setOnlineUsers] = useState<OnlineUser[]>([])
  const [input, setInput] = useState("")
  const [connected, setConnected] = useState(false)
  const [hasMore, setHasMore] = useState(true)
  const [loadingHistory, setLoadingHistory] = useState(false)
  const [loadingInitial, setLoadingInitial] = useState(true)
  const [replyingTo, setReplyingTo] = useState<ChatMessage | null>(null)
  const [sending, setSending] = useState(false)
  const [showScrollBtn, setShowScrollBtn] = useState(false)
  const [uploading, setUploading] = useState(false)
  const [uploadError, setUploadError] = useState<string | null>(null)
  const [echoIds] = useState(() => new Set<string>())
  const { user } = useAuth()
  const currentUserId = user?.id || ""

  const scrollRef = useRef<HTMLDivElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const wsRef = useRef<ArenaChatWebSocket | null>(null)
  const bottomRef = useRef<HTMLDivElement>(null)

  const canWrite = capabilities.length > 0 // member of project

  // ── fetch history ──
  const loadHistory = useCallback(async (before?: number) => {
    setLoadingHistory(true)
    try {
      const page: ChatHistoryPage = await fetchChatHistory(projectId, {
        before,
        limit: 50,
      })
      setMessages((prev) => {
        const existingIds = new Set(prev.map((m) => m.id))
        const fresh = page.messages.filter((m) => !existingIds.has(m.id))
        if (!before) return fresh // initial load
        return [...fresh, ...prev] // prepend older
      })
      setHasMore(page.has_more)
    } catch {
      // ignore
    } finally {
      setLoadingHistory(false)
      setLoadingInitial(false)
    }
  }, [projectId])

  // ── scroll helpers ──
  const isNearBottom = useCallback(() => {
    const el = scrollRef.current
    if (!el) return true
    return el.scrollHeight - el.scrollTop - el.clientHeight < 150
  }, [])

  const scrollToBottom = useCallback(() => {
    bottomRef.current?.scrollIntoView()
    setShowScrollBtn(false)
  }, [])

  // ── handle WS events ──
  const handleEvent = useCallback((event: ChatWsEvent) => {
    switch (event.type) {
      case "message": {
        if (!event.message) return
        const msg = event.message
        setMessages((prev) => {
          // Match the optimistic placeholder by its echo_id, or an existing
          // row by real id. Both predicates are PER-MESSAGE — a global "does
          // any echo match" check would rewrite every row to this same msg,
          // producing duplicate React keys.
          const matches = (m: ChatMessage) =>
            m.id === msg.id ||
            (msg.echo_id != null && m.echo_id === msg.echo_id)

          if (prev.some(matches)) {
            // Replace the matched optimistic/duplicate row in place.
            return prev.map((m) => (matches(m) ? msg : m))
          }
          return [...prev, msg]
        })
        // Scroll if near bottom
        setTimeout(() => {
          if (isNearBottom()) scrollToBottom()
          else setShowScrollBtn(true)
        }, 50)
        break
      }
      case "presence": {
        if (event.online_users) setOnlineUsers(event.online_users)
        break
      }
    }
  }, [isNearBottom, scrollToBottom])

  // ── start/stop monitoring scroll ──
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    const onScroll = () => {
      if (isNearBottom()) setShowScrollBtn(false)
    }
    el.addEventListener("scroll", onScroll, { passive: true })
    return () => el.removeEventListener("scroll", onScroll)
  }, [isNearBottom])

  // ── init ──
  //
  // The WebSocket must be created **synchronously** in the effect body
  // (not inside `loadHistory().then(...)`) so the cleanup function can
  // reliably reach it. If WS creation is deferred to a `.then()` callback,
  // a synchronous remount (React StrictMode in dev, Fast Refresh on
  // code edits, parent re-renders that flip the deps) can run the
  // cleanup BEFORE the first `.then()` resolves — leaving
  // `wsRef.current === null` for the cleanup to destroy, and letting
  // BOTH the old and new WS connections stay open. The Rust side then
  // broadcasts a "joined the chat" system message to each open
  // subscription, so the user sees the announcement twice (once per
  // surviving connection).
  useEffect(() => {
    setMessages([])
    setOnlineUsers([])
    setReplyingTo(null)
    setInput("")
    setHasMore(true)
    setLoadingInitial(true)
    echoIds.clear()

    const ws = new ArenaChatWebSocket(projectId, handleEvent, setConnected)
    wsRef.current = ws
    ws.connect()

    loadHistory()

    return () => {
      wsRef.current?.destroy()
      wsRef.current = null
    }
  }, [projectId, loadHistory, handleEvent, echoIds])

  // ── infinite scroll up ──
  const handleScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el || loadingHistory || !hasMore) return
    if (el.scrollTop < 50) {
      const oldest = messages[0]
      if (oldest) loadHistory(oldest.created_at)
    }
  }, [loadingHistory, hasMore, messages, loadHistory])

  // ── send message ──
  const handleSend = useCallback(async (e?: FormEvent) => {
    e?.preventDefault()
    if (!input.trim() || sending || !canWrite) return

    const content = input.trim()
    setInput("")
    setSending(true)

    const echoId = createTempId()
    echoIds.add(echoId)

    const optimistic: ChatMessage = {
      id: echoId,
      project_id: projectId,
      user_id: currentUserId,
      display_name: "You",
      content,
      content_type: "text",
      reply_to_id: replyingTo?.id ?? null,
      file_id: null,
      file_name: null,
      file_mime: null,
      content_attributes: {},
      status: "sending",
      echo_id: echoId,
      replied_to: replyingTo
        ? {
            user_id: replyingTo.user_id,
            display_name: replyingTo.display_name,
            content_preview: (replyingTo.content || "").slice(0, 200),
          }
        : null,
      created_at: Date.now(),
    }

    setMessages((prev) => [...prev, optimistic])
    setReplyingTo(null)
    setTimeout(() => scrollToBottom(), 100)

    wsRef.current?.sendMessage({
      type: "message",
      content,
      content_type: "text",
      reply_to_id: replyingTo?.id ?? null,
      echo_id: echoId,
    })

    setSending(false)

    // Timeout: mark as failed if no echo
    setTimeout(() => {
      setMessages((prev) =>
        prev.map((m) =>
          m.echo_id === echoId && m.status === "sending"
            ? { ...m, status: "failed" }
            : m,
        ),
      )
      echoIds.delete(echoId)
    }, 10_000)
  }, [input, sending, canWrite, replyingTo, projectId, scrollToBottom, echoIds, currentUserId])

  // ── file upload ──
  const handleFileChange = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file || uploading) return
    setUploading(true)
    try {
      const node = await uploadChatFile(projectId, file)
      const echoId = createTempId()
      echoIds.add(echoId)

      const optimistic: ChatMessage = {
        id: echoId,
        project_id: projectId,
        user_id: currentUserId,
        display_name: "You",
        content: "",
        content_type: "file",
        reply_to_id: null,
        file_id: node.id,
        file_name: node.name,
        file_mime: node.mime_type ?? null,
        content_attributes: {},
        status: "sending",
        echo_id: echoId,
        replied_to: null,
        created_at: Date.now(),
      }

      setMessages((prev) => [...prev, optimistic])
      setTimeout(() => scrollToBottom(), 100)

      wsRef.current?.sendMessage({
        type: "message",
        content: "",
        content_type: "file",
        file_id: node.id,
        echo_id: echoId,
      })

      setTimeout(() => {
        setMessages((prev) =>
          prev.map((m) =>
            m.echo_id === echoId && m.status === "sending"
              ? { ...m, status: "failed" }
              : m,
          ),
        )
        echoIds.delete(echoId)
      }, 10_000)
    } catch (err) {
      setUploadError(err instanceof Error ? err.message : "Upload failed")
      setTimeout(() => setUploadError(null), 4000)
    } finally {
      setUploading(false)
      if (fileInputRef.current) fileInputRef.current.value = ""
    }
  }, [uploading, projectId, scrollToBottom, echoIds, currentUserId])

  // ── keyboard ──
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault()
        handleSend()
      }
      if (e.key === "Escape") setReplyingTo(null)
    },
    [handleSend],
  )

  const groups = useMemo(() => groupMessagesByDate(messages), [messages])

  return (
    <div className="flex h-full min-h-0 flex-col bg-[#0d0d0d]">
      {/* Header */}
      <div className="flex h-9 items-center gap-2 border-b border-[#373737] bg-[#151515] px-3 shrink-0">
        <span className="text-xs font-medium uppercase text-[#b4b4b4]">Chat</span>
        <span className="flex items-center gap-1.5 ml-auto text-xs text-[#787878]">
          <span className={cn("h-1.5 w-1.5 rounded-full", connected ? "bg-[#4caf50]" : "bg-[#f44336]")} />
          {onlineUsers.length > 0 ? `${onlineUsers.length} online` : connected ? "Connected" : "Disconnected"}
        </span>
      </div>

      {/* Messages */}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="min-h-0 flex-1 overflow-y-auto px-3 py-2"
      >
        {loadingInitial ? (
          <div className="flex items-center justify-center h-full">
            <Loader2 className="h-5 w-5 animate-spin text-[#787878]" />
          </div>
        ) : messages.length === 0 ? (
          <div className="flex h-full items-center justify-center text-xs text-[#787878]">
            No messages yet. Start the conversation.
          </div>
        ) : (
          <div>
            {loadingHistory && (
              <div className="flex justify-center py-2">
                <Loader2 className="h-4 w-4 animate-spin text-[#787878]" />
              </div>
            )}
            {!hasMore && (
              <div className="text-center py-3 text-[11px] text-[#555]">
                Beginning of conversation
              </div>
            )}
            {groups.map((group) => (
              <div key={group.date.toISOString()}>
                <div className="flex items-center gap-3 py-3">
                  <span className="h-px flex-1 bg-[#242424]" />
                  <span className="text-[11px] font-medium text-[#787878]">
                    {formatDate(group.date)}
                  </span>
                  <span className="h-px flex-1 bg-[#242424]" />
                </div>
                {group.messages.map((msg) => (
                  <ChatBubble
                    key={msg.id}
                    message={msg}
                    // "Mine" means I authored it: either it still carries an
                    // echo_id I created locally (optimistic row or its own
                    // echo), or its user_id matches me. The server broadcasts
                    // echo_id to ALL clients, so echo_id alone is not enough —
                    // it must be one *I* generated (tracked in echoIds).
                    isMine={
                      (msg.echo_id != null && echoIds.has(msg.echo_id)) ||
                      (currentUserId !== "" && msg.user_id === currentUserId)
                    }
                    projectId={projectId}
                    onReply={setReplyingTo}
                  />
                ))}
              </div>
            ))}
            <div ref={bottomRef} />
          </div>
        )}
      </div>

      {/* Scroll-to-bottom button */}
      {showScrollBtn && (
        <div className="absolute bottom-28 left-1/2 -translate-x-1/2">
          <button
            onClick={scrollToBottom}
            className="rounded-full border border-[#373737] bg-[#1f1f1f] p-1.5 text-[#b4b4b4] hover:bg-[#2a2a2a]"
          >
            <ArrowDown className="h-3.5 w-3.5" />
          </button>
        </div>
      )}

      {/* Footer */}
      <div className="border-t border-[#373737] bg-[#151515] p-2 shrink-0">
        {/* Upload error */}
        {uploadError && (
          <div className="mb-1.5 flex items-center gap-1.5 rounded-md border border-[#f44336]/40 bg-[#2d1b1b] px-2 py-1 text-xs text-[#f44336]">
            <AlertCircle className="h-3 w-3 shrink-0" />
            {uploadError}
          </div>
        )}

        {/* Reply indicator */}
        {replyingTo && (
          <div className="mb-1.5">
            <ReplyChip
              repliedTo={{
                display_name: replyingTo.display_name,
                content: replyingTo.content,
              }}
              onCancel={() => setReplyingTo(null)}
            />
          </div>
        )}

        {/* Input row */}
        <form onSubmit={handleSend} className="flex items-end gap-1.5">
          <input
            ref={fileInputRef}
            type="file"
            className="hidden"
            onChange={handleFileChange}
            accept="image/*,application/pdf,.tex,.md,.txt,.csv,.json,.zip,.py,.ts,.tsx,.js,.jsx,.rs,.go,.java,.c,.cpp,.h,.sh,.yaml,.yml,.toml,.ipynb"
          />
          <button
            type="button"
            disabled={!canWrite || uploading}
            onClick={() => fileInputRef.current?.click()}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-[#373737] bg-[#1f1f1f] text-[#787878] hover:bg-[#2a2a2a] hover:text-[#e8e8e8] disabled:cursor-not-allowed disabled:opacity-40"
            title="Attach file"
          >
            {uploading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Paperclip className="h-4 w-4" />
            )}
          </button>
          <Textarea
            value={input}
            readOnly={!canWrite}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={canWrite ? "Type a message...  Enter to send" : "Read-only"}
            className="min-h-9 max-h-24 flex-1 resize-none border-[#373737] bg-[#1f1f1f] text-sm text-[#e8e8e8] placeholder:text-[#787878]"
          />
          <Button
            type="submit"
            disabled={!canWrite || sending || !input.trim()}
            size="icon"
            className="h-9 w-9 shrink-0 rounded-md bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]"
          >
            <Send className="h-4 w-4" />
          </Button>
        </form>
      </div>
    </div>
  )
}
