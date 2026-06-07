export type Message = {
  id: string
  role: "user" | "assistant" | "system"
  content: string
  streaming?: boolean
  timeline?: AssistantTimelineItem[]
}

export type ToolCallEntry = {
  id?: string
  name: string
  arguments: Record<string, unknown>
  output: string
  status: "running" | "success" | "error"
}

export type ChatTokenUsage = {
  conversation_id: string
  prompt_tokens: number
  completion_tokens: number
}

export type AssistantTimelineItem =
  | { id: string; type: "thinking"; text: string; expanded: boolean }
  | { id: string; type: "text"; content: string }
  | { id: string; type: "tool"; toolCall: ToolCallEntry }
  | { id: string; type: "usage"; usage: ChatTokenUsage }
  | { id: string; type: "stopped" }
  | { id: string; type: "error"; message: string }

export type ChatStreamLikeEvent = {
  conversation_id: string
  seq?: number
  content: string
  done: boolean
}

export function createTimelineId(prefix: string) {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `${prefix}-${crypto.randomUUID()}`
  }
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

export function textTimelineItem(content: string): AssistantTimelineItem {
  return { id: createTimelineId("text"), type: "text", content }
}

export function assistantTimelineFromContent(content: string): AssistantTimelineItem[] {
  return content.trim().length > 0 ? [textTimelineItem(content)] : []
}

export function appendTextTimeline(timeline: AssistantTimelineItem[], content: string): AssistantTimelineItem[] {
  if (!content) return timeline
  const last = timeline[timeline.length - 1]
  if (last?.type === "text") {
    return [
      ...timeline.slice(0, -1),
      { ...last, content: `${last.content}${content}` },
    ]
  }
  return [...timeline, textTimelineItem(content)]
}

export function appendThinkingTimeline(timeline: AssistantTimelineItem[], content: string): AssistantTimelineItem[] {
  const last = timeline[timeline.length - 1]
  if (last?.type === "thinking") {
    return [
      ...timeline.slice(0, -1),
      { ...last, text: `${last.text}${content}` },
    ]
  }
  return [
    ...timeline,
    { id: createTimelineId("thinking"), type: "thinking", text: content, expanded: false },
  ]
}

export function upsertUsageTimeline(timeline: AssistantTimelineItem[], usage: ChatTokenUsage): AssistantTimelineItem[] {
  const existingIndex = timeline.findIndex((item) => item.type === "usage")
  const usageItem: AssistantTimelineItem = { id: createTimelineId("usage"), type: "usage", usage }
  if (existingIndex < 0) return [...timeline, usageItem]
  return timeline.map((item, index) =>
    index === existingIndex && item.type === "usage"
      ? { ...item, usage }
      : item,
  )
}

export function upsertToolTimeline(timeline: AssistantTimelineItem[], entry: ToolCallEntry): AssistantTimelineItem[] {
  const existingIndex = timeline.findIndex((item) => {
    if (item.type !== "tool") return false
    if (entry.id && item.toolCall.id === entry.id) return true
    return !entry.id && item.toolCall.name === entry.name && item.toolCall.status === "running"
  })

  if (existingIndex < 0) {
    return [...timeline, { id: entry.id || createTimelineId("tool"), type: "tool", toolCall: entry }]
  }

  return timeline.map((item, index) =>
    index === existingIndex && item.type === "tool"
      ? {
          ...item,
          id: entry.id || item.id,
          toolCall: { ...item.toolCall, ...entry },
        }
      : item,
  )
}

export function appendStoppedTimeline(timeline: AssistantTimelineItem[]): AssistantTimelineItem[] {
  if (timeline.some((item) => item.type === "stopped")) return timeline
  return [...timeline, { id: createTimelineId("stopped"), type: "stopped" }]
}

export function appendErrorTimeline(timeline: AssistantTimelineItem[], message: string): AssistantTimelineItem[] {
  if (timeline.some((item) => item.type === "error" && item.message === message)) return timeline
  return [...timeline, { id: createTimelineId("error"), type: "error", message }]
}

export function hasAssistantError(message: Message | undefined): boolean {
  return message?.role === "assistant" && !!message.timeline?.some((item) => item.type === "error")
}

export function hasRenderableTimeline(message: Message): boolean {
  return message.timeline?.some((item) => {
    if (item.type === "text") return item.content.length > 0
    if (item.type === "thinking") return item.text.length > 0
    return true
  }) ?? false
}

export function updateActiveAssistant(
  prev: Message[],
  update: (message: Message) => Message,
  options: { createIfMissing?: boolean } = {},
): Message[] {
  const last = prev[prev.length - 1]
  if (!last || last.role !== "assistant" || !last.streaming) {
    if (options.createIfMissing === false) return prev
    const next = update({
      id: createTimelineId("assistant"),
      role: "assistant",
      content: "",
      streaming: true,
      timeline: [],
    })
    return [...prev, next]
  }
  return [...prev.slice(0, -1), update(last)]
}

export function applyStreamEvent(
  prev: Message[],
  event: ChatStreamLikeEvent,
  wasStopRequested: boolean,
): Message[] {
  if (event.done && !event.content) {
    const last = prev[prev.length - 1]
    if (!last || last.role !== "assistant" || !last.streaming) return prev
  }

  return updateActiveAssistant(prev, (message) => {
    let timeline = appendTextTimeline(message.timeline || [], event.content)
    if (event.done && wasStopRequested) timeline = appendStoppedTimeline(timeline)
    return {
      ...message,
      content: `${message.content}${event.content}`,
      streaming: !event.done,
      timeline,
    }
  })
}

export function appendAssistantError(prev: Message[], message: string): Message[] {
  const last = prev[prev.length - 1]
  if (hasAssistantError(last)) return prev

  if (last?.role === "assistant") {
    return [
      ...prev.slice(0, -1),
      {
        ...last,
        streaming: false,
        timeline: appendErrorTimeline(last.timeline || [], message),
      },
    ]
  }

  return [...prev, { id: createTimelineId("system"), role: "system", content: message }]
}
