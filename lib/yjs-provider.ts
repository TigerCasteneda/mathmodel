import * as Y from "yjs"
import { getToken, getWebSocketBase } from "@/lib/api"

export interface SyncMessage {
  type: "sync_update" | "sync_full" | "awareness"
  update?: number[]
  state?: number[]
}

// Awareness protocol types (subset of y-protocols/awareness)
export interface AwarenessProtocol {
  on(
    event: "update",
    handler: (
      changes: {
        added: number[]
        updated: number[]
        removed: number[]
      },
      origin: unknown,
    ) => void,
  ): void
  off(
    event: "update",
    handler: (
      changes: {
        added: number[]
        updated: number[]
        removed: number[]
      },
      origin: unknown,
    ) => void,
  ): void
  getStates(): Map<number, Record<string, unknown>>
  getLocalState(): Record<string, unknown> | null
  setLocalState(state: Record<string, unknown> | null): void
  setLocalStateField(field: string, value: unknown): void
  /** Encode awareness update for a list of changed client IDs */
  encodeAwarenessUpdate(
    clients: number[],
    states: Map<number, Record<string, unknown>>,
  ): Uint8Array
  /** Apply an incoming awareness update */
  applyAwarenessUpdate(
    update: Uint8Array,
    origin: unknown,
  ): void
}

export class YjsWebsocketProvider {
  private ws: WebSocket | null = null
  private doc: Y.Doc
  private fileId: string
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private destroyed = false
  private updateHandler: (update: Uint8Array, origin: unknown) => void
  private awareness: AwarenessProtocol | null = null
  private awarenessUpdateHandler:
    | ((changes: {
        added: number[]
        updated: number[]
        removed: number[]
      }, origin: unknown) => void)
    | null = null
  private _synced = false

  constructor(
    doc: Y.Doc,
    fileId: string,
    awareness?: AwarenessProtocol,
  ) {
    this.doc = doc
    this.fileId = fileId
    this.awareness = awareness ?? null

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

    // Bind awareness if provided
    if (this.awareness) {
      this.awarenessUpdateHandler = (
        changes: { added: number[]; updated: number[]; removed: number[] },
        origin: unknown,
      ) => {
        if (origin === this) return
        if (
          changes.added.length === 0 &&
          changes.updated.length === 0 &&
          changes.removed.length === 0
        )
          return
        const changedClients = [
          ...changes.added,
          ...changes.updated,
          ...changes.removed,
        ]
        if (changedClients.length === 0) return
        if (this.ws?.readyState === WebSocket.OPEN) {
          const states = this.awareness!.getStates()
          const encoded = this.awareness!.encodeAwarenessUpdate(
            changedClients,
            states,
          )
          const msg: SyncMessage = {
            type: "awareness",
            state: Array.from(encoded),
          }
          this.ws.send(JSON.stringify(msg))
        }
      }
      this.awareness.on("update", this.awarenessUpdateHandler)
    }

    this.connect()
    this.doc.on("update", this.updateHandler)
  }

  get synced() {
    return this._synced
  }

  private async connect() {
    if (this.destroyed) return
    const token = getToken()
    if (!token) {
      console.warn("[YjsWS] missing auth token")
      return
    }

    const base = await getWebSocketBase()
    if (this.destroyed) return
    const url = `${base}/sync?file_id=${encodeURIComponent(this.fileId)}&token=${encodeURIComponent(token)}`
    this.ws = new WebSocket(url)

    this.ws.onopen = () => {
      console.log("[YjsWS] connected", this.fileId)
    }

    this.ws.onmessage = (event) => {
      try {
        const msg: SyncMessage = JSON.parse(event.data)

        // Handle content sync
        if (msg.type === "sync_full" && msg.state) {
          const state = new Uint8Array(msg.state)
          Y.applyUpdate(this.doc, state, this)
          this._synced = true
          return
        }

        if (msg.type === "sync_update" && msg.update) {
          const update = new Uint8Array(msg.update)
          Y.applyUpdate(this.doc, update, this)
          return
        }

        // Handle awareness
        if (msg.type === "awareness" && msg.state && this.awareness) {
          const update = new Uint8Array(msg.state)
          this.awareness.applyAwarenessUpdate(update, this)
        }
      } catch (e) {
        console.error("[YjsWS] parse error", e)
      }
    }

    this.ws.onclose = () => {
      this._synced = false
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
    this._synced = false
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.doc.off("update", this.updateHandler)
    if (this.awareness && this.awarenessUpdateHandler) {
      this.awareness.off("update", this.awarenessUpdateHandler)
    }
    this.ws?.close()
  }
}
