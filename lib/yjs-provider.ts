import * as Y from "yjs"
import { getToken } from "@/lib/api"

export interface SyncMessage {
  type: "sync_update" | "sync_full" | "awareness"
  update?: number[]
  state?: number[]
}

export class YjsWebsocketProvider {
  private ws: WebSocket | null = null
  private doc: Y.Doc
  private fileId: string
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private destroyed = false
  private updateHandler: (update: Uint8Array, origin: unknown) => void

  constructor(doc: Y.Doc, fileId: string) {
    this.doc = doc
    this.fileId = fileId

    this.updateHandler = (update: Uint8Array, origin: unknown) => {
      if (origin === this) return
      if (this.ws?.readyState === WebSocket.OPEN) {
        const msg: SyncMessage = {
          type: "sync_update",
          update: Array.from(update),
        }
        this.ws.send(JSON.stringify(msg))
      }
    }

    this.connect()
    this.doc.on("update", this.updateHandler)
  }

  private connect() {
    if (this.destroyed) return
    const token = getToken()
    if (!token) {
      console.warn("[YjsWS] missing auth token")
      return
    }

    const base = process.env.NEXT_PUBLIC_WS_URL || "ws://localhost:3001"
    const url = `${base}/sync?file_id=${encodeURIComponent(this.fileId)}&token=${encodeURIComponent(token)}`
    this.ws = new WebSocket(url)

    this.ws.onopen = () => {
      console.log("[YjsWS] connected", this.fileId)
    }

    this.ws.onmessage = (event) => {
      try {
        const msg: SyncMessage = JSON.parse(event.data)
        if ((msg.type === "sync_full" && msg.state) || (msg.type === "sync_update" && msg.update)) {
          const data = msg.state || msg.update!
          const update = new Uint8Array(data)
          Y.applyUpdate(this.doc, update, this)
        }
      } catch (e) {
        console.error("[YjsWS] parse error", e)
      }
    }

    this.ws.onclose = () => {
      if (!this.destroyed) {
        console.log("[YjsWS] disconnected, reconnecting in 2s")
        this.reconnectTimer = setTimeout(() => this.connect(), 2000)
      }
    }

    this.ws.onerror = (err) => {
      console.error("[YjsWS] error", err)
    }
  }

  destroy() {
    this.destroyed = true
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.doc.off("update", this.updateHandler)
    this.ws?.close()
  }
}
