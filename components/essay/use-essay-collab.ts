"use client"

import { useEffect, useRef } from "react"
import * as Y from "yjs"
import { YjsWebsocketProvider } from "@/lib/yjs-provider"
import type { AwarenessProtocol } from "@/lib/yjs-provider"
import { getToken } from "@/lib/api"
import { isTauri } from "@/lib/tauri-api"
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
 * Manages the Yjs document lifecycle for one essay file.
 * Tauri local mode uses an in-memory Y.Doc; authenticated web mode connects
 * the same doc to the CRDT WebSocket backend.
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
  const fileIdRef = useRef<string | null>(null)
  const seededForFileRef = useRef<string | null>(null)
  const onSyncedRef = useRef(onSynced)

  onSyncedRef.current = onSynced

  // Y.Doc.guid is random, so keep the app file id in a separate ref.
  if (!ydocRef.current || fileIdRef.current !== fileId) {
    providerRef.current?.destroy()
    ydocRef.current?.destroy()

    ydocRef.current = new Y.Doc()
    fileIdRef.current = fileId
    seededForFileRef.current = null
    awarenessRef.current = null
    providerRef.current = null
    syncedRef.current = false
  }

  const ydoc = ydocRef.current
  const ytext = ydoc.getText("content")

  if (
    initialContent !== undefined &&
    ytext.toString() === "" &&
    seededForFileRef.current !== fileId
  ) {
    ytext.insert(0, initialContent)
    seededForFileRef.current = fileId
  }

  useEffect(() => {
    const token = getToken()

    if (!token || isTauri()) {
      syncedRef.current = true
      onSyncedRef.current?.()
      awarenessRef.current = null
      providerRef.current = null
      return
    }

    let awareness: AwarenessProtocol | null = null
    let provider: YjsWebsocketProvider | null = null
    let checkSynced: ReturnType<typeof setInterval> | null = null
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

      checkSynced = setInterval(() => {
        if (!provider?.synced) return

        syncedRef.current = true
        if (checkSynced) clearInterval(checkSynced)
        checkSynced = null

        if (
          ydoc.getText("content").toString() === "" &&
          initialContent !== undefined &&
          seededForFileRef.current !== fileId
        ) {
          ydoc.getText("content").insert(0, initialContent)
          seededForFileRef.current = fileId
        }

        onSyncedRef.current?.()
      }, 100)
    })

    return () => {
      cancelled = true
      if (checkSynced) clearInterval(checkSynced)
      provider?.destroy()
    }
  }, [fileId, readOnly, ydoc, initialContent])

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
