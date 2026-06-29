"use client"

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  Suspense,
} from "react"
import { useParams, useRouter, useSearchParams } from "next/navigation"
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels"
import { EssayEditor } from "@/components/essay/essay-editor"
import type { EssayEditorHandle } from "@/components/essay/essay-editor"
import { EssayTopBar } from "@/components/essay/essay-topbar"
import { EssaySidebar } from "@/components/essay/essay-sidebar"
import { EssayCommentsPanel } from "@/components/essay/essay-comments"
import { EssayStatusBar } from "@/components/essay/essay-statusbar"
import { EssayTabStrip, type EssayTab } from "@/components/essay/essay-tab-strip"
import { EssayOutlinePanel } from "@/components/essay/essay-outline-panel"
import { EssayBacklinksPanel } from "@/components/essay/essay-backlinks-panel"
import { useEssayCollab } from "@/components/essay/use-essay-collab"
import { setLocalUserInfo } from "@/lib/codemirror/awareness"
import type { AwarenessUserInfo } from "@/lib/codemirror/awareness"
import { isTauri, listFiles, readFile } from "@/lib/tauri-api"
import { getProjectFileContent, getToken } from "@/lib/api"
import type { EssayComment } from "@/lib/codemirror/comments"
import { useAuth } from "@/hooks/use-auth"
import { WikilinkIndex } from "@/lib/wikilink-index"

type SyncState = "synced" | "saving" | "offline"
type SidebarTab = "files" | "outline" | "backlinks" | "comments"

function essayUserInfoKey(userId: string) {
  return userId ? `essay-user-info:${userId}` : "essay-user-info:anon"
}

function getOrCreateUserInfo(userId: string, displayName: string): AwarenessUserInfo {
  const key = essayUserInfoKey(userId)
  if (typeof window !== "undefined") {
    const stored = localStorage.getItem(key)
    if (stored) {
      try {
        return JSON.parse(stored)
      } catch {
        // fall through
      }
    }
  }
  const colors = [
    "#f87171", "#60a5fa", "#34d399", "#fbbf24",
    "#a78bfa", "#f472b6", "#38bdf8", "#fb923c",
  ]
  const color = colors[Math.floor(Math.random() * colors.length)]
  const info: AwarenessUserInfo = {
    name: displayName || "User",
    color,
    colorLight: color + "33",
  }
  if (typeof window !== "undefined") {
    localStorage.setItem(key, JSON.stringify(info))
  }
  return info
}

/** Strip `.md`/`.markdown` from a path/id and return the bare basename. */
function basenameOf(input: string): string {
  const last = input.split("/").pop() ?? input
  return last.replace(/\.(md|markdown)$/i, "")
}

function EssayPageContent() {
  const params = useParams<{ id: string }>()
  const searchParams = useSearchParams()
  const router = useRouter()
  const { user } = useAuth()
  const sessionUserId = user?.id ?? ""

  const projectId = params.id
  const fileId = searchParams.get("file")
  const filePath = searchParams.get("path")
  const fileKey = fileId ?? filePath ?? ""
  const currentBasename = fileKey ? basenameOf(fileKey) : ""

  const [title, setTitle] = useState("Untitled Essay")
  const [loadedFile, setLoadedFile] = useState<{ key: string; content: string } | null>(null)
  const [loading, setLoading] = useState(true)
  const [syncState, setSyncState] = useState<SyncState>("offline")
  const [wordCount, setWordCount] = useState(0)
  const [sectionInfo, setSectionInfo] = useState({ index: 1, count: 1 })
  const [lastSaved, setLastSaved] = useState<Date | null>(null)
  const [collaborators, setCollaborators] = useState<
    Array<{ name: string; color: string }>
  >([])
  const [notFound, setNotFound] = useState(false)
  const [sidebarTab, setSidebarTab] = useState<SidebarTab>("files")

  // Multi-note tab state. `openTabs` is the list of currently open
  // notes; the URL `file`/`path` params drive which one is rendered.
  const [openTabs, setOpenTabs] = useState<EssayTab[]>([])
  const [dirtyTabIds, setDirtyTabIds] = useState<Set<string>>(new Set())

  // Set of md basenames (no `.md`) that exist in the project. Updated
  // by the file tree and consumed by the editor's wikilink decorations
  // + autocomplete to know which `[[…]]` targets are resolved.
  const [knownFiles, setKnownFiles] = useState<Set<string>>(new Set())

  // Wikilink index — client-only, persisted per-project. Used by the
  // Backlinks panel to answer "what links to the current note?".
  const wikilinkIndex = useMemo(() => {
    if (!projectId) return null
    const idx = new WikilinkIndex(projectId)
    idx.hydrate()
    return idx
  }, [projectId])

  // Expose the underlying CodeMirror EditorView so the outline panel
  // can scroll to a heading line.
  const editorRef = useRef<EssayEditorHandle | null>(null)

  const userInfo = getOrCreateUserInfo(sessionUserId, user?.display_name ?? "")
  const token = getToken()

  // Track open tabs: every time the URL's file changes, push it onto
  // the list (or set as active if already present). Closes use
  // `closeTab` below to navigate to a sibling.
  useEffect(() => {
    if (!fileKey) return
    const basename = basenameOf(fileKey)
    setOpenTabs((prev) => {
      if (prev.some((t) => t.id === basename)) return prev
      const name = `${basename}.md`
      return [...prev, { id: basename, name }]
    })
  }, [fileKey])

  const closeTab = useCallback(
    (tabId: string) => {
      setOpenTabs((prev) => {
        const idx = prev.findIndex((t) => t.id === tabId)
        if (idx === -1) return prev
        const next = prev.filter((t) => t.id !== tabId)
        // If the closed tab was the active one, navigate to the
        // previous sibling (or next if there is no previous). If no
        // tabs remain, navigate back to the project root.
        if (tabId === currentBasename) {
          if (next.length === 0) {
            router.push(`/projects/${projectId}`)
          } else {
            const fallback = next[Math.max(0, idx - 1)] ?? next[0]
            const params = new URLSearchParams()
            params.set("file", `${fallback.id}.md`)
            params.set("path", `${fallback.id}.md`)
            router.push(`/projects/${projectId}/essay?${params.toString()}`)
          }
        }
        return next
      })
      setDirtyTabIds((prev) => {
        if (!prev.has(tabId)) return prev
        const next = new Set(prev)
        next.delete(tabId)
        return next
      })
    },
    [currentBasename, projectId, router],
  )

  const selectTab = useCallback(
    (tabId: string) => {
      if (tabId === currentBasename) return
      const params = new URLSearchParams()
      params.set("file", `${tabId}.md`)
      params.set("path", `${tabId}.md`)
      router.push(`/projects/${projectId}/essay?${params.toString()}`)
    },
    [currentBasename, projectId, router],
  )

  // Load file content and metadata before mounting editor
  useEffect(() => {
    if (!fileId && !filePath) {
      setNotFound(true)
      setLoading(false)
      return
    }

    let cancelled = false
    const nextFileKey = fileId ?? filePath ?? ""
    setLoading(true)
    setNotFound(false)
    setLoadedFile(null)

    async function load() {
      try {
        if (isTauri() && filePath) {
          const name = filePath.split("/").pop() || filePath
          setTitle(name.replace(/\.md$/, ""))
          const content = await readFile(filePath)
          if (!cancelled) setLoadedFile({ key: nextFileKey, content })
        } else if (isTauri() && fileId) {
          const name = fileId.split("/").pop() || fileId
          setTitle(name.replace(/\.md$/, ""))
          const content = await readFile(fileId)
          if (!cancelled) setLoadedFile({ key: nextFileKey, content })
        } else if (fileId) {
          const response = await getProjectFileContent(projectId, fileId)
          if (!cancelled) setLoadedFile({ key: nextFileKey, content: response.content })
        } else {
          if (cancelled) return
          setLoadedFile({ key: nextFileKey, content: "" })
        }
      } catch (err) {
        console.warn("[essay] could not load file content:", err)
        if (!cancelled) setLoadedFile({ key: nextFileKey, content: "" })
      }
      if (!cancelled) setLoading(false)
    }

    load()
    return () => { cancelled = true }
  }, [projectId, fileId, filePath])

  // Set up collaboration (only after content is loaded)
  const { ydoc, ytext, awareness, commentsMap } = useEssayCollab({
    fileId: fileKey,
    initialContent: loadedFile?.key === fileKey ? loadedFile.content : undefined,
    readOnly: !token,
    onSynced: () => setSyncState("synced"),
  })

  // Set local user info once awareness is ready
  useEffect(() => {
    if (awareness) {
      setLocalUserInfo(awareness, userInfo)
      const interval = setInterval(() => {
        const states = awareness.getStates()
        const others: Array<{ name: string; color: string }> = []
        states.forEach((state, clientId) => {
          if (clientId !== (awareness as any).doc?.clientID) {
            const user = state.user as AwarenessUserInfo | undefined
            if (user) others.push({ name: user.name, color: user.color })
          }
        })
        setCollaborators(others)
      }, 2000)
      return () => clearInterval(interval)
    }
  }, [awareness, userInfo])

  // Re-index wikilinks whenever the file changes — capture the
  // current text into the index so the Backlinks panel can answer
  // queries immediately.
  useEffect(() => {
    if (!wikilinkIndex || !loadedFile || loadedFile.key !== fileKey) return
    wikilinkIndex.scan(loadedFile.content, currentBasename)
    wikilinkIndex.persist()
  }, [wikilinkIndex, loadedFile, fileKey, currentBasename])

  // Debounced re-index on every content change — scan only the
  // current note; the index's diff logic avoids redundant updates.
  const handleContentChange = useCallback(
    (content: string) => {
      const words = content
        .replace(/[#*`~>\[\]()!_|]/g, " ")
        .replace(/\$\$[\s\S]*?\$\$/g, " ")
        .replace(/\$[^$]*\$/g, " ")
        .split(/\s+/)
        .filter(Boolean)
      setWordCount(words.length)

      const headings = content.match(/^#{1,6}\s/gm)
      setSectionInfo({
        index: 1,
        count: headings ? headings.length + 1 : 1,
      })

      setSyncState("saving")
      const timer = setTimeout(() => {
        setSyncState("synced")
        setLastSaved(new Date())
      }, 1000)

      // Wikilink re-index — fire and forget, debounced implicitly by
      // the editor's own update throttling.
      if (wikilinkIndex) {
        wikilinkIndex.scan(content, currentBasename)
        wikilinkIndex.persist()
      }

      return () => clearTimeout(timer)
    },
    [wikilinkIndex, currentBasename],
  )

  // Tab-strip dirty indicator
  const handleDirtyChange = useCallback((dirty: boolean) => {
    setDirtyTabIds((prev) => {
      const next = new Set(prev)
      if (dirty) next.add(currentBasename)
      else next.delete(currentBasename)
      return next
    })
  }, [currentBasename])

  // Click a `[[wikilink]]` widget — open the target in a new tab.
  const handleWikilinkNavigate = useCallback(
    (target: string) => {
      const params = new URLSearchParams()
      params.set("file", `${target}.md`)
      params.set("path", `${target}.md`)
      router.push(`/projects/${projectId}/essay?${params.toString()}`)
    },
    [projectId, router],
  )

  const handleScrollToComment = useCallback((comment: EssayComment) => {
    console.log("[essay] scroll to comment:", comment.id)
  }, [])

  const handleRename = useCallback((newTitle: string) => {
    setTitle(newTitle)
  }, [])

  // Keyboard shortcut to exit
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key === "E") {
        e.preventDefault()
        router.push(`/projects/${projectId}`)
      }
    }
    window.addEventListener("keydown", handler)
    return () => window.removeEventListener("keydown", handler)
  }, [projectId, router])

  if (notFound) {
    return (
      <div className="flex h-screen items-center justify-center bg-essay-bg">
        <div className="text-center">
          <h1 className="text-2xl font-semibold text-essay-text mb-2">
            File not found
          </h1>
          <p className="text-sm text-essay-text-faint mb-4">
            The requested essay file does not exist or you do not have access.
          </p>
          <button
            onClick={() => router.push(`/projects/${projectId}`)}
            className="text-sm text-essay-link hover:underline"
          >
            ← Back to project
          </button>
        </div>
      </div>
    )
  }

  if (loading || loadedFile?.key !== fileKey) {
    return (
      <div className="flex h-screen items-center justify-center bg-essay-bg">
        <div className="text-sm text-essay-text-faint">Loading…</div>
      </div>
    )
  }

  // Decorate open tabs with their dirty flag
  const tabsWithDirty: EssayTab[] = openTabs.map((t) => ({
    ...t,
    dirty: dirtyTabIds.has(t.id),
  }))

  const editorView = editorRef.current?.getView() ?? null

  return (
    <div className="flex h-screen flex-col bg-essay-bg overflow-hidden">
      <EssayTopBar
        title={title}
        projectId={projectId}
        syncState={syncState}
        collaborators={collaborators}
        onRename={handleRename}
      />

      {/* Multi-note tab strip — sits between the top bar and the editor */}
      <EssayTabStrip
        tabs={tabsWithDirty}
        activeTabId={currentBasename || null}
        onSelect={selectTab}
        onClose={closeTab}
      />

      <PanelGroup direction="horizontal" className="flex-1">
        {/* Editor Panel */}
        <Panel defaultSize={70} minSize={40}>
          <EssayEditor
            ref={editorRef}
            key={fileKey}
            ydoc={ydoc}
            ytext={ytext}
            awareness={awareness}
            commentsMap={commentsMap}
            fileId={fileKey}
            essayFileName={filePath ?? undefined}
            readOnly={!token}
            onChange={handleContentChange}
            onDirtyChange={handleDirtyChange}
            knownFiles={knownFiles}
            onWikilinkNavigate={handleWikilinkNavigate}
          />
        </Panel>

        <PanelResizeHandle className="w-1 bg-essay-border hover:bg-essay-border-strong transition-colors" />

        {/* Sidebar Panel */}
        <Panel defaultSize={30} minSize={15} maxSize={40}>
          <div className="flex flex-col h-full bg-essay-bg-sidebar border-l border-essay-border">
            {/* Tab bar — Obsidian-style uppercase */}
            <div className="essay-sidebar-tabs">
              {([
                { key: "files", label: "Files" },
                { key: "outline", label: "Outline" },
                { key: "backlinks", label: "Backlinks" },
                { key: "comments", label: "Comments" },
              ] as { key: SidebarTab; label: string }[]).map(({ key, label }) => (
                <button
                  key={key}
                  className={`essay-sidebar-tab ${
                    sidebarTab === key ? "essay-sidebar-tab--active" : ""
                  }`}
                  onClick={() => setSidebarTab(key)}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="flex-1 min-h-0 overflow-y-auto">
              {sidebarTab === "files" ? (
                <EssaySidebar
                  projectId={projectId}
                  fileId={fileId ?? filePath ?? ""}
                  onKnownFilesChanged={setKnownFiles}
                />
              ) : sidebarTab === "outline" ? (
                <EssayOutlinePanel editorView={editorView} />
              ) : sidebarTab === "backlinks" ? (
                <EssayBacklinksPanel
                  projectId={projectId}
                  currentBasename={currentBasename}
                  index={wikilinkIndex}
                />
              ) : (
                <EssayCommentsPanel
                  commentsMap={commentsMap}
                  onScrollTo={handleScrollToComment}
                />
              )}
            </div>
          </div>
        </Panel>
      </PanelGroup>

      <EssayStatusBar
        wordCount={wordCount}
        sectionIndex={sectionInfo.index}
        sectionCount={sectionInfo.count}
        lastSaved={lastSaved}
      />
    </div>
  )
}

export default function EssayPage() {
  return (
    <Suspense
      fallback={
        <div className="flex h-screen items-center justify-center bg-essay-bg">
          <div className="text-sm text-essay-text-faint">Loading…</div>
        </div>
      }
    >
      <EssayPageContent />
    </Suspense>
  )
}