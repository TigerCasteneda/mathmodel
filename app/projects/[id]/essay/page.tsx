"use client"

import { useEffect, useState, useCallback, Suspense } from "react"
import { useParams, useRouter, useSearchParams } from "next/navigation"
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels"
import { EssayEditor } from "@/components/essay/essay-editor"
import { EssayTopBar } from "@/components/essay/essay-topbar"
import { EssaySidebar } from "@/components/essay/essay-sidebar"
import { EssayCommentsPanel } from "@/components/essay/essay-comments"
import { EssayStatusBar } from "@/components/essay/essay-statusbar"
import { useEssayCollab } from "@/components/essay/use-essay-collab"
import { setLocalUserInfo } from "@/lib/codemirror/awareness"
import type { AwarenessUserInfo } from "@/lib/codemirror/awareness"
import { isTauri, listFiles, readFile } from "@/lib/tauri-api"
import { getToken } from "@/lib/api"
import type { EssayComment } from "@/lib/codemirror/comments"

type SyncState = "synced" | "saving" | "offline"
type SidebarTab = "files" | "comments"

function getOrCreateUserInfo(): AwarenessUserInfo {
  const key = "essay-user-info"
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
    name: "User",
    color,
    colorLight: color + "33",
  }
  if (typeof window !== "undefined") {
    localStorage.setItem(key, JSON.stringify(info))
  }
  return info
}

function EssayPageContent() {
  const params = useParams<{ id: string }>()
  const searchParams = useSearchParams()
  const router = useRouter()

  const projectId = params.id
  const fileId = searchParams.get("file")
  const filePath = searchParams.get("path")

  const [title, setTitle] = useState("Untitled Essay")
  const [initialContent, setInitialContent] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [syncState, setSyncState] = useState<SyncState>("offline")
  const [wordCount, setWordCount] = useState(0)
  const [sectionInfo, setSectionInfo] = useState({
    index: 1,
    count: 1,
  })
  const [lastSaved, setLastSaved] = useState<Date | null>(null)
  const [collaborators, setCollaborators] = useState<
    Array<{ name: string; color: string }>
  >([])
  const [notFound, setNotFound] = useState(false)
  const [sidebarTab, setSidebarTab] = useState<SidebarTab>("files")

  const userInfo = getOrCreateUserInfo()
  const token = getToken()

  // Load file content and metadata before mounting editor
  useEffect(() => {
    if (!fileId && !filePath) {
      setNotFound(true)
      setLoading(false)
      return
    }

    let cancelled = false

    async function load() {
      try {
        // Derive title
        if (isTauri() && filePath) {
          const name = filePath.split("/").pop() || filePath
          setTitle(name.replace(/\.md$/, ""))

          // Load file content from Tauri filesystem
          const content = await readFile(filePath)
          if (!cancelled) {
            setInitialContent(content)
          }
        } else {
          // For server mode, try to get title from file tree
          if (isTauri()) {
            const tree = await listFiles()
            if (cancelled) return
            const findFile = (
              node: typeof tree,
            ): { name: string } | null => {
              if ((node as any).id === fileId) {
                return { name: node.name }
              }
              if (node.children) {
                for (const c of node.children) {
                  const found = findFile(c)
                  if (found) return found
                }
              }
              return null
            }
            const found = findFile(tree)
            if (found) {
              setTitle(found.name.replace(/\.md$/, ""))
            }
          }

          // For server mode: content loaded via Yjs sync from server
          if (!cancelled) setInitialContent("")
        }
      } catch (err) {
        // File read failed — start with empty editor; don't treat as "not found"
        console.warn("[essay] could not load file content:", err)
        if (!cancelled) {
          setInitialContent("")
        }
      }
      if (!cancelled) setLoading(false)
    }

    load()
    return () => { cancelled = true }
  }, [fileId, filePath])

  // Set up collaboration (only after content is loaded)
  const { ydoc, ytext, awareness, commentsMap } = useEssayCollab({
    fileId: fileId ?? filePath ?? "",
    initialContent: initialContent ?? undefined,
    readOnly: !isTauri() && !token,
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
            if (user) {
              others.push({ name: user.name, color: user.color })
            }
          }
        })
        setCollaborators(others)
      }, 2000)

      return () => clearInterval(interval)
    }
  }, [awareness, userInfo])

  // Compute word count and section info
  const handleContentChange = useCallback((content: string) => {
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
    return () => clearTimeout(timer)
  }, [])

  const handleScrollToComment = useCallback(
    (comment: EssayComment) => {
      try {
        // Convert relative position to absolute and dispatch into editor
        // The editor view is not directly accessible from here, but
        // we can use the editor ref approach. For now, clicking the
        // comment card switches to files tab to view the highlight.
        // Full scroll-to is a follow-up (needs editor ref).
        console.log("[essay] scroll to comment:", comment.id)
      } catch {
        // ignore
      }
    },
    [],
  )

  const handleRename = useCallback(
    (newTitle: string) => {
      setTitle(newTitle)
      console.log("[essay] rename requested:", newTitle)
    },
    [],
  )

  // Handle keyboard shortcut to exit
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

  // Loading state
  if (loading) {
    return (
      <div className="flex h-screen items-center justify-center bg-[#0d0d0d]">
        <div className="text-sm text-[#666]">Loading...</div>
      </div>
    )
  }

  // Not found state
  if (notFound) {
    return (
      <div className="flex h-screen items-center justify-center bg-[#0d0d0d]">
        <div className="text-center">
          <h1 className="text-2xl font-semibold text-[#e0e0e0] mb-2">
            File not found
          </h1>
          <p className="text-sm text-[#666] mb-4">
            The requested essay file does not exist or you do not have access.
          </p>
          <button
            onClick={() => router.push(`/projects/${projectId}`)}
            className="text-sm text-[#569cd6] hover:underline"
          >
            ← Back to project
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-screen flex-col bg-[#0d0d0d] overflow-hidden">
      <EssayTopBar
        title={title}
        projectId={projectId}
        syncState={syncState}
        collaborators={collaborators}
        onRename={handleRename}
      />

      <PanelGroup direction="horizontal" className="flex-1">
        {/* Editor Panel */}
        <Panel defaultSize={70} minSize={40}>
          <EssayEditor
            ydoc={ydoc}
            ytext={ytext}
            awareness={awareness}
            commentsMap={commentsMap}
            readOnly={!isTauri() && !token}
            onChange={handleContentChange}
          />
        </Panel>

        <PanelResizeHandle className="w-1 bg-[#2a2a2a] hover:bg-[#444] transition-colors active:bg-[#569cd6]" />

        {/* Sidebar Panel */}
        <Panel defaultSize={30} minSize={15} maxSize={40}>
          <div className="flex flex-col h-full bg-[#0d0d0d] border-l border-[#2a2a2a]">
            {/* Tab bar */}
            <div className="flex h-8 border-b border-[#2a2a2a] shrink-0">
              {(["files", "comments"] as SidebarTab[]).map((tab) => (
                <button
                  key={tab}
                  className={`flex-1 text-[10px] font-semibold uppercase tracking-wider border-b-2 transition-colors ${
                    sidebarTab === tab
                      ? "border-[#569cd6] text-[#ccc]"
                      : "border-transparent text-[#555] hover:text-[#888]"
                  }`}
                  onClick={() => setSidebarTab(tab)}
                >
                  {tab === "files" ? "Files" : "Comments"}
                </button>
              ))}
            </div>
            {/* Content */}
            {sidebarTab === "files" ? (
              <EssaySidebar projectId={projectId} fileId={fileId ?? filePath ?? ""} />
            ) : (
              <EssayCommentsPanel
                commentsMap={commentsMap}
                onScrollTo={handleScrollToComment}
              />
            )}
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

// Wrap in Suspense for useSearchParams
export default function EssayPage() {
  return (
    <Suspense
      fallback={
        <div className="flex h-screen items-center justify-center bg-[#0d0d0d]">
          <div className="text-sm text-[#666]">Loading...</div>
        </div>
      }
    >
      <EssayPageContent />
    </Suspense>
  )
}
