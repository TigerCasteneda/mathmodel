"use client"

import { useEffect, useRef } from "react"
import * as Y from "yjs"
import { YjsWebsocketProvider } from "@/lib/yjs-provider"
import type { AwarenessProtocol } from "@/lib/yjs-provider"

interface UseEssayCollabOptions {
  fileId: string
  initialContent?: string
  readOnly?: boolean
  onSynced?: () => void
}

interface UseEssayCollabResult {
  ydoc: Y.Doc
  ytext: Y.Text
  awareness: AwarenessProtocol | null
  provider: YjsWebsocketProvider | null
  synced: boolean
}

/**
 * Hook that manages the Yjs document lifecycle for an essay file.
 * Creates a Y.Doc, connects via WebSocket, and returns the shared types.
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

  // Create once per fileId
  if (!ydocRef.current || ydocRef.current.guid !== fileId) {
    // Destroy previous
    providerRef.current?.destroy()
    ydocRef.current?.destroy()

    const doc = new Y.Doc()
    // Seed with initial content if Y.Text is empty
    const ytext = doc.getText("content")
    if (initialContent && ytext.toString() === "") {
      ytext.insert(0, initialContent)
    }

    ydocRef.current = doc
    syncedRef.current = false
  }

  const ydoc = ydocRef.current
  const ytext = ydoc.getText("content")

  useEffect(() => {
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

      // Signal synced once we get the first full state
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

  return {
    ydoc,
    ytext,
    awareness: awarenessRef.current,
    provider: providerRef.current,
    synced: syncedRef.current,
  }
}
