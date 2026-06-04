"use client"

import { FormEvent, useEffect, useMemo, useState } from "react"
import { Bot, Send, UserRound } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import { aiChat, loadSession, onChatError, onChatStream } from "@/lib/tauri-api"

type Message = {
  id: string
  role: "user" | "assistant" | "system"
  content: string
  streaming?: boolean
}

export function ChatPanel({ conversationId = "default" }: { conversationId?: string }) {
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState("")
  const [sending, setSending] = useState(false)
  const [loaded, setLoaded] = useState(false)

  // Load persisted session on mount / conversation change
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
    }).catch(() => {
      setMessages([])
      setLoaded(true)
    })
  }, [conversationId])

  useEffect(() => {
    const offStream = onChatStream((event) => {
      if (event.conversation_id !== conversationId) return
      setMessages((prev) => {
        const next = [...prev]
        const last = next[next.length - 1]
        if (!last || last.role !== "assistant" || !last.streaming) {
          next.push({
            id: crypto.randomUUID(),
            role: "assistant",
            content: event.content,
            streaming: !event.done,
          })
          return next
        }
        last.content += event.content
        last.streaming = !event.done
        return next
      })
      if (event.done) setSending(false)
    })

    const offError = onChatError((event) => {
      if (event.conversation_id !== conversationId) return
      setMessages((prev) => [
        ...prev,
        {
          id: crypto.randomUUID(),
          role: "system",
          content: event.message,
        },
      ])
      setSending(false)
    })

    return () => {
      offStream()
      offError()
    }
  }, [conversationId])

  const canSend = useMemo(() => input.trim().length > 0 && !sending, [input, sending])

  const handleSubmit = async (event?: FormEvent) => {
    event?.preventDefault()
    const message = input.trim()
    if (!message || sending) return
    setInput("")
    setSending(true)
    setMessages((prev) => [
      ...prev,
      { id: crypto.randomUUID(), role: "user", content: message },
    ])
    try {
      await aiChat(message, conversationId)
    } catch (err) {
      setMessages((prev) => [
        ...prev,
        {
          id: crypto.randomUUID(),
          role: "system",
          content: err instanceof Error ? err.message : "Chat request failed.",
        },
      ])
      setSending(false)
    }
  }

  return (
    <section className="flex h-full min-h-0 flex-col bg-[#0d0d0d] text-[#e8e8e8]">
      <div className="flex h-10 items-center gap-2 border-b border-[#373737] px-3">
        <Bot className="h-4 w-4 text-[#d4a574]" />
        <span className="text-sm font-medium">Modeler AI</span>
        <span className="ml-auto text-xs text-[#787878]">Native Tauri</span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-5">
        {messages.length === 0 ? (
          <div className="mx-auto flex h-full max-w-2xl flex-col justify-center">
            <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-md bg-[#232323] text-[#d4a574]">
              <Bot className="h-6 w-6" />
            </div>
            <h2 className="text-xl font-semibold">Modeler AI</h2>
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-4">
            {messages.map((message) => (
              <div
                key={message.id}
                className={cn(
                  "flex gap-3",
                  message.role === "user" && "justify-end",
                )}
              >
                {message.role !== "user" && (
                  <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[#232323] text-[#d4a574]">
                    <Bot className="h-4 w-4" />
                  </div>
                )}
                <div
                  className={cn(
                    "max-w-[78%] whitespace-pre-wrap rounded-2xl px-4 py-3 text-sm leading-6",
                    message.role === "user"
                      ? "bg-[#d4a574] text-[#111111]"
                      : message.role === "system"
                        ? "border border-[#f44336]/40 bg-[#2d1b1b] text-[#ffb4b4]"
                        : "bg-[#232323] text-[#e8e8e8]",
                  )}
                >
                  {message.content}
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

      <form onSubmit={handleSubmit} className="border-t border-[#373737] bg-[#121212] p-3">
        <div className="mx-auto flex max-w-3xl items-end gap-2 rounded-2xl border border-[#373737] bg-[#232323] p-2">
          <Textarea
            value={input}
            onChange={(event) => setInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault()
                handleSubmit()
              }
            }}
            placeholder="Message Modeler AI..."
            className="min-h-12 flex-1 resize-none border-0 bg-transparent text-sm text-[#e8e8e8] shadow-none placeholder:text-[#787878] focus-visible:ring-0"
          />
          <Button
            type="submit"
            size="icon"
            disabled={!canSend}
            className="h-9 w-9 rounded-full bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]"
          >
            <Send className="h-4 w-4" />
          </Button>
        </div>
      </form>
    </section>
  )
}
