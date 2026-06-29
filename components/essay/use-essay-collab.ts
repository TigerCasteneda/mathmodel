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
  /**
   * Discard the in-memory Y.Doc for `fileId`. Called when the user
   * closes a tab so we don't leak Y.Docs forever — every tab strip
   * close eventually evicts its doc. Returns true if a doc was found
   * and destroyed.
   */
  evict: (fileId: string) => boolean
}

/**
 * Manages the Yjs document lifecycle for one essay file.
 *
 * **Tauri local mode** uses an in-memory Y.Doc; **authenticated web mode**
 * connects that doc to the CRDT WebSocket backend.
 *
 * **Cache strategy.** Previously, switching files destroyed the
 * previous file's Y.Doc, so switching back to a tab lost every edit
 * made in that tab since the last disk write (Tauri mode has no
 * provider, so the Y.Doc was the only source of unsaved edits).
 *
 * We now keep a `Map<fileId, Y.Doc>` cache. Switching files just
 * swaps which entry is "active"; the old doc stays alive. A doc is
 * only destroyed when its tab is closed (via `evict()`) or evicted
 * by the LRU cap (default: 8 docs, ~tens of MB max).
 *
 * Per-doc seeding state (`seededFor:<fileId>`) ensures we don't
 * re-insert `initialContent` on subsequent renders of the same file
 * even after switching away and back.
 */
const MAX_CACHED_DOCS = 8

export function useEssayCollab({
  fileId,
  initialContent,
  readOnly,
  onSynced,
}: UseEssayCollabOptions): UseEssayCollabResult {
  const cacheRef = useRef<Map<string, Y.Doc>>(new Map())
  const providersRef = useRef<Map<string, YjsWebsocketProvider>>(new Map())
  const awarenessRef = useRef<Map<string, AwarenessProtocol>>(new Map())
  const seededRef = useRef<Set<string>>(new Set())
  const orderRef = useRef<string[]>([]) // LRU: most-recently-used at the end
  const syncedRef = useRef(false)
  const onSyncedRef = useRef(onSynced)

  onSyncedRef.current = onSynced

  // Get or create the Y.Doc for the active fileId.
  let activeDoc = cacheRef.current.get(fileId)
  if (!activeDoc) {
    activeDoc = new Y.Doc()
    cacheRef.current.set(fileId, activeDoc)
    orderRef.current.push(fileId)
    // LRU eviction when the cache grows beyond the cap.
    while (orderRef.current.length > MAX_CACHED_DOCS) {
      const evictId = orderRef.current.shift()
      if (!evictId) break
      const evictDoc = cacheRef.current.get(evictId)
      if (evictDoc) {
        providersRef.current.get(evictId)?.destroy()
        providersRef.current.delete(evictId)
        awarenessRef.current.delete(evictId)
        evictDoc.destroy()
        cacheRef.current.delete(evictId)
        seededRef.current.delete(evictId)
      }
    }
  } else {
    // Promote to MRU position.
    const idx = orderRef.current.indexOf(fileId)
    if (idx >= 0) orderRef.current.splice(idx, 1)
    orderRef.current.push(fileId)
  }

  const ydoc = activeDoc
  const ytext = ydoc.getText("content")

  // Seed initial content the first time we see this file with a
  // non-empty initialContent and an empty ytext.
  if (
    initialContent !== undefined &&
    ytext.toString() === "" &&
    !seededRef.current.has(fileId)
  ) {
    ytext.insert(0, initialContent)
    seededRef.current.add(fileId)
  }

  // Evict helper — destroys the cached doc for a closed tab.
  const evict = (id: string): boolean => {
    const doc = cacheRef.current.get(id)
    if (!doc) return false
    providersRef.current.get(id)?.destroy()
    providersRef.current.delete(id)
    awarenessRef.current.delete(id)
    doc.destroy()
    cacheRef.current.delete(id)
    seededRef.current.delete(id)
    const idx = orderRef.current.indexOf(id)
    if (idx >= 0) orderRef.current.splice(idx, 1)
    return true
  }

  useEffect(() => {
    const token = getToken()
    const cachedProvider = providersRef.current.get(fileId)
    if (cachedProvider) {
      // Re-attach to an existing provider for this file (e.g. coming
      // back from another tab).
      syncedRef.current = cachedProvider.synced
      return
    }

    if (!token || isTauri()) {
      syncedRef.current = true
      onSyncedRef.current?.()
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
      awarenessRef.current.set(fileId, awareness)
      provider = new YjsWebsocketProvider(ydoc, fileId, awareness)
      providersRef.current.set(fileId, provider)

      if (readOnly) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ;(awareness as any).setLocalState(null)
      }

      checkSynced = setInterval(() => {
        if (!provider?.synced) return

        syncedRef.current = true
        if (checkSynced) clearInterval(checkSynced)
        checkSynced = null

        // Same seeding logic as offline — only on first sync, when
        // the server hasn't given us content yet.
        if (
          ydoc.getText("content").toString() === "" &&
          initialContent !== undefined &&
          !seededRef.current.has(fileId)
        ) {
          ydoc.getText("content").insert(0, initialContent)
          seededRef.current.add(fileId)
        }

        onSyncedRef.current?.()
      }, 100)
    })

    return () => {
      cancelled = true
      if (checkSynced) clearInterval(checkSynced)
      // Note: we DO NOT destroy the provider / doc here. The cache
      // outlives the effect so switching files back and forth doesn't
      // churn Yjs state. The doc is only destroyed via `evict()` when
      // the user closes the tab.
    }
  }, [fileId, readOnly, ydoc, initialContent])

  const commentsMap = ydoc.getMap<EssayComment>("comments")
  const provider = providersRef.current.get(fileId) ?? null
  const awareness = awarenessRef.current.get(fileId) ?? null

  return {
    ydoc,
    ytext,
    commentsMap,
    awareness,
    provider,
    synced: syncedRef.current,
    evict,
  }
}