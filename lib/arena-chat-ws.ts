import { getToken, getWebSocketBase, type ChatMessage, type OnlineUser } from "@/lib/api"

export interface ChatWsMessage {
  type: "message" | "ping"
  content?: string
  content_type?: "text" | "file"
  reply_to_id?: string | null
  file_id?: string | null
  content_attributes?: Record<string, unknown>
  echo_id?: string
}

export interface ChatWsEvent {
  type: "message" | "presence" | "heartbeat_ack"
  message?: ChatMessage
  online_users?: OnlineUser[]
  echo_id?: string
}

export type ChatEventCallback = (event: ChatWsEvent) => void
export type ConnectionCallback = (connected: boolean) => void

export class ArenaChatWebSocket {
  private ws: WebSocket | null = null
  private projectId: string
  private onEvent: ChatEventCallback
  private onConnectionChange?: ConnectionCallback
  private pingTimer: ReturnType<typeof setInterval> | null = null
  private heartbeatTimer: ReturnType<typeof setTimeout> | null = null
  private destroyed = false
  private reconnectAttempts = 0
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private maxReconnectDelay = 30_000

  constructor(
    projectId: string,
    onEvent: ChatEventCallback,
    onConnectionChange?: ConnectionCallback,
  ) {
    this.projectId = projectId
    this.onEvent = onEvent
    this.onConnectionChange = onConnectionChange
  }

  async connect(): Promise<void> {
    if (this.destroyed) return
    try {
      const base = await getWebSocketBase()
      const token = getToken()
      const url = `${base}/projects/${encodeURIComponent(this.projectId)}/arena/chat/ws?token=${encodeURIComponent(token ?? "")}`
      this.ws = new WebSocket(url)

      this.ws.onopen = () => {
        this.reconnectAttempts = 0
        this.onConnectionChange?.(true)
        this.startPing()
        this.resetHeartbeat()
      }

      this.ws.onmessage = (ev) => {
        this.resetHeartbeat()
        try {
          const event = JSON.parse(ev.data as string) as ChatWsEvent
          this.onEvent(event)
        } catch {
          // ignore malformed
        }
      }

      this.ws.onclose = () => {
        this.onConnectionChange?.(false)
        this.stopPing()
        this.clearHeartbeat()
        if (!this.destroyed) this.scheduleReconnect()
      }

      this.ws.onerror = () => {
        // onclose will fire after this
      }
    } catch {
      if (!this.destroyed) this.scheduleReconnect()
    }
  }

  sendMessage(msg: ChatWsMessage): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg))
    }
  }

  destroy(): void {
    this.destroyed = true
    this.stopPing()
    this.clearHeartbeat()
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.ws) {
      this.ws.onclose = null
      this.ws.close()
      this.ws = null
    }
  }

  private startPing(): void {
    this.stopPing()
    this.pingTimer = setInterval(() => {
      this.sendMessage({ type: "ping" })
    }, 25_000)
  }

  private stopPing(): void {
    if (this.pingTimer) {
      clearInterval(this.pingTimer)
      this.pingTimer = null
    }
  }

  private resetHeartbeat(): void {
    this.clearHeartbeat()
    this.heartbeatTimer = setTimeout(() => {
      // No message received in 35s — force close to trigger reconnect
      this.ws?.close()
    }, 35_000)
  }

  private clearHeartbeat(): void {
    if (this.heartbeatTimer) {
      clearTimeout(this.heartbeatTimer)
      this.heartbeatTimer = null
    }
  }

  private scheduleReconnect(): void {
    if (this.destroyed) return
    const delay = Math.min(
      1000 * 2 ** this.reconnectAttempts,
      this.maxReconnectDelay,
    )
    this.reconnectAttempts += 1
    this.reconnectTimer = setTimeout(() => {
      if (!this.destroyed) this.connect()
    }, delay)
  }
}
