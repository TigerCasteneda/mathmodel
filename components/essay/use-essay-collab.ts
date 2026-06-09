"use client"

import { useEffect, useRef } from "react"
import * as Y from "yjs"
import { YjsWebsocketProvider } from "@/lib/yjs-provider"
import type { AwarenessProtocol } from "@/lib/yjs-provider"
import { getToken } from "@/lib/api"
import type { EssayComment } from "@/lib/codemirror/comments"

interface UseEssayCollabOptions {
  fileId: string
  initialContent?: string
  readOnly?: boolean
  onSynced?: () => void
}

interface UseEssayCollabResult {
  ydoc: Y.Doc
  ytext: Y.Text
  commentsMap: Y.Map<EssayComment>
  awareness: AwarenessProtocol | null
  provider: YjsWebsocketProvider | null
  synced: boolean
}

/**
 * Hook that manages the Yjs document lifecycle for an essay file.
 * In Tauri local mode (no auth token), uses Y.Doc in-memory without WebSocket.
 * When a valid token exists, connects via WebSocket for real-time collaboration.
 */
export function useEssayCollab({
  fileId,
  initialContent,
  readOnly,
  onSynced,
}: UseEssayCollabOptions): UseEssayCollabResult {
  const ydocRef = useRef<Y.Doc | null>(null)
  const providerRef = useRef<YjsWebsocketProvider | null>(null)
  const awarenessRef = useRef<AwarenessProtocol | null>(null)
  const syncedRef = useRef(false)
  const seededRef = useRef(false)

  // Create once per fileId — seed initial content BEFORE any sync
  if (!ydocRef.current || ydocRef.current.guid !== fileId) {
    providerRef.current?.destroy()
    ydocRef.current?.destroy()

    const doc = new Y.Doc()
    const ytext = doc.getText("content")

    // Seed initial content NOW, before any provider connects.
    // The provider's SyncFull will overwrite if the server has content;
    // if there's no server, this is the permanent content.
    if (initialContent !== undefined && ytext.toString() === "" && !seededRef.current) {
      ytext.insert(0, initialContent)
      seededRef.current = true
    }

    ydocRef.current = doc
    syncedRef.current = false
  } else {
    // Same doc — if initialContent changed and doc is empty, seed
    const ytext = ydocRef.current.getText("content")
    if (initialContent !== undefined && ytext.toString() === "" && !seededRef.current) {
      ytext.insert(0, initialContent)
      seededRef.current = true
    }
  }

  const ydoc = ydocRef.current
  const ytext = ydoc.getText("content")

  useEffect(() => {
    const token = getToken()
    // No token = local-only mode, skip WebSocket entirely
    if (!token) {
      syncedRef.current = true
      onSynced?.()
      awarenessRef.current = null
      providerRef.current = null
      return
    }

    let awareness: AwarenessProtocol | null = null
    let provider: YjsWebsocketProvider | null = null
    let cancelled = false

    void import("y-protocols/awareness").then((mod) => {
      if (cancelled) return
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      awareness = new (mod as any).Awareness(ydoc) as AwarenessProtocol
      awarenessRef.current = awareness
      provider = new YjsWebsocketProvider(ydoc, fileId, awareness)
      providerRef.current = provider

      if (readOnly) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ;(awareness as any).setLocalState(null)
      }

      const checkSynced = setInterval(() => {
        if (provider?.synced) {
          syncedRef.current = true
          clearInterval(checkSynced)
          onSynced?.()
        }
      }, 100)
    })

    return () => {
      cancelled = true
      provider?.destroy()
    }
  }, [fileId, readOnly, onSynced, ydoc])

  const commentsMap = ydoc.getMap<EssayComment>("comments")

  return {
    ydoc,
    ytext,
    commentsMap,
    awareness: awarenessRef.current,
    provider: providerRef.current,
    synced: syncedRef.current,
  }
}
