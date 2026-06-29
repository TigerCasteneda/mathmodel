"use client"

import { useEffect, useMemo, useRef, useState } from "react"
import { useRouter } from "next/navigation"
import { AlertCircle, Archive, BookOpen, Check, CheckCircle2, ChevronDown, ChevronRight, Copy, Database, FileCode, FileImage, FileText, Folder, FolderOpen, Globe2, Library, Link, Loader2, LogOut, MessageSquare, Network, MonitorUp, MonitorX, PencilLine, Play, RefreshCw, RotateCcw, Save, Search, Settings, SidebarIcon, Trash2, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { ArenaPanel } from "@/components/arena/arena-panel"
import { KnowledgeBasePanel } from "@/components/knowledge/knowledge-base-panel"
import { ChatPanel } from "@/components/chat/chat-panel"
import { CodeEditor } from "@/components/editor/code-editor"
import ImageViewer from "@/components/editor/image-viewer"
import PdfViewer from "@/components/editor/pdf-viewer"
import { cn } from "@/lib/utils"
import { ModelerMark } from "@/components/chat/modeler-mark"
import {
  archiveSession,
  compileLatex,
  deleteSession,
  getAiConfigStatus,
  getSidecarStatus,
  listSessions,
  openUrl,
  renameSession,
  searchSessions,
  unarchiveSession,
  onFileChange,
  researchAnalyzeUrl,
  researchExtractAndSave,
  researchSearchNative,
  setAiConfig,
  type AiConfigStatus,
  type AgentSource,
  type FileTreeItem,
  type NativeResearchSearchItem,
  type ResearchSearchKind,
  type ResearchScraper,
  type SessionInfo,
} from "@/lib/tauri-api"
import { useTauriAgent } from "@/hooks/use-tauri-agent"
import { useScreenShare } from "@/hooks/use-screen-share"
import {
  ALL_PROJECT_CAPABILITIES,
  createArenaCard,
  createProjectFile,
  createProjectInvite,
  deleteProjectFile,
  getToken,
  getProject,
  getProjectFileContent,
  listProjectMembers,
  listProjectInvites,
  listProjectTree,
  listResearchItems,
  parseCapabilities,
  removeProjectMember,
  revokeProjectInvite,
  updateProjectMember,
  type Project,
  type ProjectCapability,
  type ProjectInvite,
  type ProjectMember,
  updateProjectFileContent,
  type ProjectFileTreeItem,
  type ProjectRole,
  type ResearchItem,
} from "@/lib/api"
import { researchItemToArenaInput, searchResultToArenaInput } from "@/lib/research-to-arena"
import { AgentResearchView } from "@/components/research/agent-research-view"
import { useAuth } from "@/hooks/use-auth"

type Activity = "explorer" | "arena" | "knowledge" | "research" | "chat" | "settings"
type Tab = {
  id: string
  title: string
  kind: "file" | "chat" | "research" | "diff"
  language?: string
  content?: string
  diff?: { left: string; right: string; leftTitle?: string; rightTitle?: string }
  dirty?: boolean
  remoteFileId?: string
  remoteUpdatedAt?: number
  readOnly?: boolean
  localExternalConflict?: boolean
  externalChanged?: boolean
  saveStatus?: "idle" | "saving" | "saved" | "error" | "conflict"
}
type WorkspaceMode = "host" | "guest"

const sampleTree: FileTreeItem = {
  name: "workspace",
  path: "",
  type: "folder",
  children: [
    { name: "model.py", path: "model.py", type: "file", language: "python" },
    { name: "README.md", path: "README.md", type: "file", language: "markdown" },
  ],
}

const sampleContent = `import numpy as np

def objective(x):
    return np.sum(np.square(x))
`

function remoteTreeToFileTree(items: ProjectFileTreeItem[]): FileTreeItem {
  const convert = (item: ProjectFileTreeItem, parentPath = ""): FileTreeItem => {
    const path = parentPath ? `${parentPath}/${item.name}` : item.name
    return {
      id: item.id,
      name: item.name,
      path,
      type: item.type,
      zone: item.zone,
      updated_at: item.updated_at,
      language: item.type === "file" ? fileLanguage({ name: item.name, path, type: item.type }) : undefined,
      children: item.children?.map((child) => convert(child, path)),
    }
  }

  return {
    name: "project",
    path: "",
    type: "folder",
    children: items.map((item) => convert(item)),
  }
}

const activities = [
  { id: "explorer" as const, icon: FileText, label: "Explorer" },
  { id: "arena" as const, icon: Network, label: "Arena" },
  { id: "knowledge" as const, icon: Library, label: "Knowledge Base" },
  { id: "research" as const, icon: BookOpen, label: "Research" },
  { id: "chat" as const, icon: MessageSquare, label: "Chat" },
  { id: "settings" as const, icon: Settings, label: "Settings" },
]

function fileLanguage(file: FileTreeItem) {
  const ext = file.name.split(".").pop()?.toLowerCase()
  if (ext === "pdf") return "pdf"
  if (ext === "png" || ext === "jpg" || ext === "jpeg") return "image"
  if (file.language) return file.language
  if (ext === "py") return "python"
  if (ext === "ts" || ext === "tsx") return "typescript"
  if (ext === "js" || ext === "jsx") return "javascript"
  if (ext === "json") return "json"
  if (ext === "md") return "markdown"
  if (ext === "tex") return "latex"
  return "plaintext"
}

type SyncStats = {
  created: number
  updated: number
  deleted: number
  skipped: number
  conflicts: SyncConflict[]
  failed: number
}

type SyncConflict = {
  path: string
  fileId: string
  remoteUpdatedAt: number
  localDeleted?: boolean
}

function emptySyncStats(): SyncStats {
  return { created: 0, updated: 0, deleted: 0, skipped: 0, conflicts: [], failed: 0 }
}

type SyncManifestEntry = {
  fileId: string
  remoteUpdatedAt: number
  localHash: string
  remoteHash: string
  localDeleted?: boolean
}

type SyncManifest = Record<string, SyncManifestEntry>

// localStorage keys for project-scoped preferences are namespaced by
// user_id so two accounts on the same browser (or sharing a project)
// don't see each other's workspace mode, auto-sync toggle, host
// folder path, or sync manifest. The path is the most sensitive —
// it leaks a real on-disk location like `C:\Users\alice\...`.
function syncManifestKey(projectId: string, userId: string) {
  return `modeler:host-sync-manifest:${userId}:${projectId}`
}

function hostFolderKey(projectId: string, userId: string) {
  return `modeler:host-folder:${userId}:${projectId}`
}

function autoSyncKey(projectId: string, userId: string) {
  return `modeler:auto-sync:${userId}:${projectId}`
}

function workspaceModeKey(projectId: string, userId: string) {
  return `modeler:workspace-mode:${userId}:${projectId}`
}

function loadSyncManifest(projectId: string, userId: string): SyncManifest {
  if (typeof window === "undefined") return {}
  try {
    const raw = localStorage.getItem(syncManifestKey(projectId, userId))
    if (!raw) return {}
    const parsed = JSON.parse(raw)
    return parsed && typeof parsed === "object" ? parsed as SyncManifest : {}
  } catch {
    return {}
  }
}

function saveSyncManifest(projectId: string, userId: string, manifest: SyncManifest) {
  if (typeof window === "undefined") return
  localStorage.setItem(syncManifestKey(projectId, userId), JSON.stringify(manifest))
}

function normalizePathKey(path: string) {
  return path.replace(/\\/g, "/")
}

function decodeCurrentUserId(): string | null {
  if (typeof window === "undefined") return null
  const token = localStorage.getItem("auth_token")
  if (!token) return null
  try {
    const payload = token.split(".")[1]?.replace(/-/g, "+").replace(/_/g, "/")
    if (!payload) return null
    const decoded = JSON.parse(window.atob(payload))
    return typeof decoded.sub === "string" ? decoded.sub : null
  } catch {
    return null
  }
}

function hashContent(content: string) {
  let hash = 2166136261
  for (let i = 0; i < content.length; i += 1) {
    hash ^= content.charCodeAt(i)
    hash = Math.imul(hash, 16777619)
  }
  return (hash >>> 0).toString(16)
}

function remoteVersion(node: ProjectFileTreeItem) {
  return node.updated_at ?? 0
}

function addSyncConflict(stats: SyncStats, conflict: SyncConflict) {
  if (!stats.conflicts.some((item) => item.path === conflict.path && item.fileId === conflict.fileId)) {
    stats.conflicts.push(conflict)
  }
}

function findRemoteChild(
  nodes: ProjectFileTreeItem[],
  name: string,
  type: "file" | "folder",
) {
  return nodes.find((node) => node.name === name && node.type === type)
}

function FileNode({
  item,
  depth,
  activePath,
  onOpen,
  onOpenEssay,
}: {
  item: FileTreeItem
  depth: number
  activePath?: string
  onOpen: (file: FileTreeItem) => void
  onOpenEssay?: (file: FileTreeItem) => void
}) {
  const [open, setOpen] = useState(depth < 1)
  const folder = item.type === "folder"
  const isMd = !folder && item.name.endsWith(".md")

  return (
    <div className="group/file">
      <button
        className={cn(
          "flex h-7 w-full items-center gap-2 px-2 text-left text-xs text-[#b4b4b4] hover:bg-[#232323]",
          activePath === item.path && "bg-[#2d2d2d] text-[#e8e8e8]",
        )}
        style={{ paddingLeft: depth * 12 + 10 }}
        onClick={() => folder ? setOpen((value) => !value) : onOpen(item)}
      >
        {folder ? (
          open ? <FolderOpen className="h-4 w-4 text-[#d4a574] shrink-0" /> : <Folder className="h-4 w-4 text-[#d4a574] shrink-0" />
        ) : (
          <FileCode className="h-4 w-4 text-[#64b5f6] shrink-0" />
        )}
        <span className="truncate flex-1">{item.name}</span>
        {isMd && onOpenEssay && (
          <span
            className="opacity-0 group-hover/file:opacity-100 transition-opacity shrink-0 p-0.5 rounded hover:bg-[#3a3a3a] cursor-pointer"
            title="Open in Essay Editor"
            role="button"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault()
                e.stopPropagation()
                onOpenEssay(item)
              }
            }}
            onClick={(e) => {
              e.stopPropagation()
              onOpenEssay(item)
            }}
          >
            <PencilLine className="h-3 w-3 text-[#d4a574]" />
          </span>
        )}
      </button>
      {folder && open && item.children?.map((child) => (
        <FileNode key={child.path || child.name} item={child} depth={depth + 1} activePath={activePath} onOpen={onOpen} onOpenEssay={onOpenEssay} />
      ))}
    </div>
  )
}

const RESEARCH_KINDS: Array<{ value: ResearchSearchKind; label: string; icon: typeof Search }> = [
  { value: "auto", label: "Auto", icon: Search },
  { value: "web", label: "Web", icon: Globe2 },
  { value: "paper", label: "Paper", icon: BookOpen },
  { value: "dataset", label: "Dataset", icon: Database },
  { value: "code", label: "Code", icon: FileCode },
  { value: "docs", label: "Docs", icon: Library },
]

const SCRAPER_OPTIONS: Array<{ value: ResearchScraper; label: string }> = [
  { value: "scrapling", label: "Scrapling" },
  { value: "firecrawl", label: "Firecrawl" },
  { value: "tavily", label: "Tavily" },
]

// Copy a source URL to the clipboard so users can paste it into a browser
// themselves — the in-app result cards are not directly clickable links.
function CopyUrlButton({ url, size = "sm" }: { url: string; size?: "sm" | "xs" }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(url)
      setCopied(true)
      setTimeout(() => setCopied(false), 1600)
    } catch {
      /* Clipboard can be unavailable in some webviews; fail silently. */
    }
  }
  const compact = size === "xs"
  return (
    <button
      type="button"
      onClick={copy}
      aria-label={copied ? "URL copied" : "Copy source URL"}
      title={url}
      className={cn(
        "inline-flex cursor-pointer items-center gap-1 rounded border transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#d4a574]",
        compact ? "px-1.5 py-0.5 text-[10px]" : "px-2 py-1 text-[11px]",
        copied
          ? "border-[#9bd6b5]/40 text-[#9bd6b5]"
          : "border-[#373737] text-[#b4b4b4] hover:border-[#d4a574]/60 hover:text-[#ebc396]",
      )}
    >
      {copied ? (
        <Check className={compact ? "h-2.5 w-2.5" : "h-3 w-3"} />
      ) : (
        <Copy className={compact ? "h-2.5 w-2.5" : "h-3 w-3"} />
      )}
      {copied ? "Copied" : "Copy URL"}
    </button>
  )
}

function errorText(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message
  if (typeof error === "string" && error.trim()) return error
  return fallback
}

function formatMirrorSummary(
  workspaceMode: WorkspaceMode | undefined,
  response: Awaited<ReturnType<typeof researchExtractAndSave>>,
): string {
  const mirror = response.local_mirror
  if (!mirror) return ""
  if (workspaceMode !== "host") return ""
  if (mirror.skipped > 0) {
    return " Open a folder in Explorer to enable local mirror."
  }
  if (mirror.errors.length > 0) {
    const errNames = mirror.errors
      .slice(0, 3)
      .map((e) => e.file_name)
      .join(", ")
    return ` Local mirror: ${mirror.created} of ${mirror.attempted} (skipped: ${errNames}).`
  }
  if (mirror.attempted > 0) {
    return ` Local mirror: ${mirror.created} of ${mirror.attempted} file(s).`
  }
  return ""
}

function ResearchSearchPanel({
  projectId,
  capabilities,
  onKeepOpen,
  workspaceMode,
  hostFolder,
}: {
  projectId: string
  capabilities: ProjectCapability[]
  onKeepOpen: () => void
  workspaceMode?: WorkspaceMode
  hostFolder?: string | null
}) {
  const [items, setItems] = useState<ResearchItem[]>([])
  const [query, setQuery] = useState("")
  const [urlInput, setUrlInput] = useState("")
  const [kind, setKind] = useState<ResearchSearchKind>("auto")
  const [scraper, setScraper] = useState<ResearchScraper>("scrapling")
  const [results, setResults] = useState<NativeResearchSearchItem[]>([])
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [loading, setLoading] = useState(false)
  const [urlAnalyzing, setUrlAnalyzing] = useState(false)
  const [loadingStage, setLoadingStage] = useState<"planning" | "searching" | "ranking">("planning")
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [arenaSendingUrl, setArenaSendingUrl] = useState<string | null>(null)
  const [mode, setMode] = useState<"classic" | "agent">("classic")
  const researchSearchIdRef = useRef<string | null>(null)
  const urlAnalyzeIdRef = useRef<string | null>(null)

  useEffect(() => {
    listResearchItems(projectId).then(setItems).catch(() => setItems([]))
  }, [projectId])

  const canSave = capabilities.includes("files.write") && capabilities.includes("ai.write")
  const selectedResults = Array.from(selected).map((index) => results[index]).filter(Boolean)

  const runSearch = async () => {
    const trimmedQuery = query.trim()
    if (!trimmedQuery) return
    const requestId = crypto.randomUUID()
    researchSearchIdRef.current = requestId
    setLoading(true)
    setLoadingStage(kind === "auto" ? "planning" : "searching")
    setMessage(null)
    setSelected(new Set())
    const timers: Array<ReturnType<typeof setTimeout>> = []
    if (kind === "auto") {
      timers.push(setTimeout(() => {
        if (researchSearchIdRef.current === requestId) setLoadingStage("searching")
      }, 700))
      timers.push(setTimeout(() => {
        if (researchSearchIdRef.current === requestId) setLoadingStage("ranking")
      }, 1800))
    } else {
      timers.push(setTimeout(() => {
        if (researchSearchIdRef.current === requestId) setLoadingStage("ranking")
      }, 1200))
    }
    try {
      const response = await researchSearchNative(trimmedQuery, kind, 16, scraper)
      if (researchSearchIdRef.current !== requestId) return
      setResults(response.results)
      setSelected(new Set(response.results.map((_, index) => index)))
      if (response.warning) setMessage(response.warning)
      else if (response.results.length === 0) setMessage("No research results found.")
    } catch (error) {
      if (researchSearchIdRef.current !== requestId) return
      setResults([])
      setMessage(errorText(error, "Research search failed."))
    } finally {
      timers.forEach(clearTimeout)
      if (researchSearchIdRef.current !== requestId) return
      setLoading(false)
    }
  }

  const toggleResult = (index: number) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }

  const analyzeUrl = async () => {
    const url = urlInput.trim()
    if (!url) return
    const requestId = crypto.randomUUID()
    urlAnalyzeIdRef.current = requestId
    setUrlAnalyzing(true)
    setMessage(null)
    try {
      const item = await researchAnalyzeUrl(url)
      if (urlAnalyzeIdRef.current !== requestId) return
      setResults((prev) => [item, ...prev])
      setSelected((prev) => new Set([0, ...Array.from(prev).map((index) => index + 1)]))
      setUrlInput("")
      setMessage("URL analyzed. Review the extracted source, then Save & Extract to add it to the Research library.")
    } catch (error) {
      if (urlAnalyzeIdRef.current !== requestId) return
      setMessage(errorText(error, "URL analysis failed."))
    } finally {
      if (urlAnalyzeIdRef.current !== requestId) return
      setUrlAnalyzing(false)
    }
  }

  const saveSelected = async () => {
    if (selectedResults.length === 0 || !canSave) return
    setSaving(true)
    setMessage(null)
    try {
      const response = await researchExtractAndSave({
        project_id: projectId,
        results: selectedResults,
        kind,
        auth_token: getToken(),
        workspace_mode: workspaceMode,
        host_folder: workspaceMode === "host" ? hostFolder ?? null : null,
      })
      const warningText = response.warnings?.length ? ` ${response.warnings.join(" ")}` : ""
      const mirrorText = formatMirrorSummary(workspaceMode, response)
      setMessage(
        `Saved ${response.saved} item(s) and created ${response.files_created} research file(s).${mirrorText}${warningText}`,
      )
      setSelected(new Set())
      onKeepOpen()
      try {
        setItems(await listResearchItems(projectId))
      } catch {
        /* Keep the saved state visible even if the recent list refresh fails. */
      }
    } catch (error) {
      setMessage(errorText(error, "Research save failed."))
    } finally {
      setSaving(false)
    }
  }

  // Save sources selected in the agentic research view. Maps AgentSource to
  // the native search-item shape and reuses the same extract-and-save flow.
  const saveAgentSources = async (sources: AgentSource[]) => {
    if (sources.length === 0 || !canSave) return
    setSaving(true)
    setMessage(null)
    try {
      const mapped: NativeResearchSearchItem[] = sources.map((s) => ({
        title: s.title,
        url: s.url,
        content: s.content,
        provider: s.provider,
        source: "agent_research",
        category: s.category,
        relevance_score: 1,
        raw_json: {},
      }))
      const response = await researchExtractAndSave({
        project_id: projectId,
        results: mapped,
        kind: "auto",
        auth_token: getToken(),
        workspace_mode: workspaceMode,
        host_folder: workspaceMode === "host" ? hostFolder ?? null : null,
      })
      const warningText = response.warnings?.length ? ` ${response.warnings.join(" ")}` : ""
      const mirrorText = formatMirrorSummary(workspaceMode, response)
      setMessage(
        `Saved ${response.saved} item(s) and created ${response.files_created} research file(s).${mirrorText}${warningText}`,
      )
      onKeepOpen()
      try {
        setItems(await listResearchItems(projectId))
      } catch {
        /* keep saved state visible even if refresh fails */
      }
    } catch (error) {
      setMessage(errorText(error, "Research save failed."))
    } finally {
      setSaving(false)
    }
  }

  // Send a single search result straight to the Arena as a card, without the
  // full Save & Extract flow. Direct field mapping, no AI call.
  const sendResultToArena = async (result: NativeResearchSearchItem) => {
    if (!canSave || arenaSendingUrl) return
    setArenaSendingUrl(result.url)
    setMessage(null)
    try {
      const input = searchResultToArenaInput(result)
      await createArenaCard(projectId, input)
      setMessage(`Added to Arena: ${input.title}`)
    } catch (error) {
      setMessage(errorText(error, "Send to Arena failed."))
    } finally {
      setArenaSendingUrl(null)
    }
  }

  // Send an already-saved reference to the Arena as a card.
  const sendItemToArena = async (item: ResearchItem) => {
    if (!canSave || arenaSendingUrl) return
    setArenaSendingUrl(item.id)
    setMessage(null)
    try {
      const input = researchItemToArenaInput(item)
      await createArenaCard(projectId, input)
      setMessage(`Added to Arena: ${input.title}`)
    } catch (error) {
      setMessage(errorText(error, "Send to Arena failed."))
    } finally {
      setArenaSendingUrl(null)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-[#0d0d0d] text-[#e8e8e8]">
      {/* Mode toggle: classic ranked list vs. agentic researcher */}
      <div className="flex items-center gap-1 border-b border-[#373737] bg-[#121212] px-3 py-2">
        <div className="flex items-center gap-1 rounded-md border border-[#373737] bg-[#1a1a1a] p-0.5">
          <button
            onClick={() => setMode("classic")}
            className={cn(
              "rounded px-2.5 py-1 text-xs font-medium",
              mode === "classic" ? "bg-[#2d241a] text-[#ebc396]" : "text-[#b4b4b4] hover:text-[#e8e8e8]",
            )}
          >
            Search
          </button>
          <button
            onClick={() => setMode("agent")}
            className={cn(
              "flex items-center gap-1 rounded px-2.5 py-1 text-xs font-medium",
              mode === "agent" ? "bg-[#2d241a] text-[#ebc396]" : "text-[#b4b4b4] hover:text-[#e8e8e8]",
            )}
          >
            <ModelerMark size={12} state={mode === "agent" ? "thinking" : "idle"} className="shrink-0" />
            Agent
          </button>
        </div>
        {mode === "agent" && (
          <span className="ml-2 text-[11px] text-[#787878]">
            Agentic researcher · streams a cited answer from academic sources
          </span>
        )}
      </div>

      {mode === "agent" ? (
        <div className="min-h-0 flex-1">
          <AgentResearchView scraper={scraper} onSaveSources={canSave ? saveAgentSources : undefined} />
        </div>
      ) : (
        <div className="flex h-full min-h-0 flex-col bg-[#0d0d0d] text-[#e8e8e8]">
          <div className="border-b border-[#373737] bg-[#121212] p-3">
            <div className="mx-auto flex max-w-5xl flex-col gap-3">
              <div className="flex flex-wrap items-center gap-2">
                {RESEARCH_KINDS.map((item) => {
                  const Icon = item.icon
                  return (
                    <button
                      key={item.value}
                      onClick={() => setKind(item.value)}
                      className={cn(
                        "flex h-8 items-center gap-1.5 rounded-md border px-2.5 text-xs",
                        kind === item.value
                          ? "border-[#d4a574] bg-[#2d241a] text-[#ebc396]"
                          : "border-[#373737] bg-[#1a1a1a] text-[#b4b4b4] hover:border-[#464646]",
                      )}
                    >
                      <Icon className="h-3.5 w-3.5" />
                      {item.label}
                    </button>
                  )
                })}
                <div
                  role="radiogroup"
                  aria-label="Search provider"
                  className="ml-auto flex items-center gap-1 rounded-md border border-[#373737] bg-[#1a1a1a] p-0.5"
                >
                  {SCRAPER_OPTIONS.map((option) => {
                    const active = scraper === option.value
                    const disabled = kind === "docs"
                    return (
                      <button
                        key={option.value}
                        type="button"
                        role="radio"
                        aria-checked={active}
                        aria-label={`Use ${option.label} scraper`}
                        title={disabled ? "Docs always use Context7" : `Search with ${option.label}`}
                        disabled={disabled}
                        onClick={() => setScraper(option.value)}
                        className={cn(
                          "flex h-7 cursor-pointer items-center rounded px-2.5 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#d4a574]",
                          active && !disabled
                            ? "bg-[#2d241a] text-[#ebc396]"
                            : "text-[#b4b4b4] hover:text-[#e8e8e8]",
                          disabled && "cursor-not-allowed opacity-40",
                        )}
                      >
                        {option.label}
                      </button>
                    )
                  })}
                </div>
              </div>
              <div className="flex gap-2">
                <div className="relative flex-1">
                  <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[#787878]" />
                  <Input
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    onKeyDown={(event) => { if (event.key === "Enter") runSearch() }}
                    placeholder={kind === "docs" ? "Search library docs, e.g. scipy optimize linprog" : "Search methods, datasets, papers, code, or competition references"}
                    className="border-[#373737] bg-[#232323] pl-9 text-sm"
                  />
                </div>
                <Button onClick={runSearch} disabled={loading || !query.trim()} className="bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]">
                  {loading ? <Loader2 className="h-4 w-4 animate-spin" /> : <Search className="h-4 w-4" />}
                </Button>
              </div>
              <div className="flex gap-2">
                <div className="relative flex-1">
                  <Link className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[#787878]" />
                  <Input
                    value={urlInput}
                    onChange={(event) => setUrlInput(event.target.value)}
                    onKeyDown={(event) => { if (event.key === "Enter") analyzeUrl() }}
                    placeholder="Paste a paper, arXiv, DOI, GitHub, Gitee, dataset, or PDF URL"
                    className="border-[#373737] bg-[#232323] pl-9 text-sm"
                  />
                </div>
                <Button onClick={analyzeUrl} disabled={urlAnalyzing || !urlInput.trim()} variant="outline" className="border-[#373737] bg-[#1a1a1a] text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8]">
                  {urlAnalyzing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Link className="mr-2 h-4 w-4" />}
                  Analyze URL
                </Button>
              </div>
              {message && (
                <div className="flex items-center gap-2 rounded-md border border-[#373737] bg-[#1a1a1a] px-3 py-2 text-xs text-[#b4b4b4]">
                  <AlertCircle className="h-3.5 w-3.5 text-[#d4a574]" />
                  {message}
                </div>
              )}
            </div>
          </div>

          <ScrollArea className="min-h-0 flex-1">
            <div className="mx-auto grid max-w-5xl gap-4 p-4 lg:grid-cols-[minmax(0,1fr)_320px]">
              <div className="min-w-0 space-y-3">
                {loading ? (
                  <div className="rounded-md border border-[#373737] bg-[#1a1a1a] p-5 text-sm text-[#b4b4b4]">
                    {loadingStage === "planning" ? "Planning research search..." : loadingStage === "searching" ? "Searching sources..." : "Ranking results..."}
                  </div>
                ) : results.length === 0 ? (
                  <div className="rounded-md border border-[#373737] bg-[#1a1a1a] p-8 text-center text-sm text-[#787878]">
                    Search results will appear here. Saving selected results automatically extracts modeling notes and creates Markdown/BibTeX files.
                  </div>
                ) : results.map((result, index) => (
                  <article
                    key={`${result.provider}-${result.url}-${index}`}
                    className={cn("rounded-md border bg-[#1a1a1a] p-3 transition-colors", selected.has(index) ? "border-[#d4a574]" : "border-[#373737]")}
                  >
                    <div className="flex items-start gap-3">
                      <button
                        onClick={() => toggleResult(index)}
                        className={cn(
                          "mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded border",
                          selected.has(index) ? "border-[#d4a574] bg-[#d4a574] text-[#111111]" : "border-[#464646] text-transparent",
                        )}
                      >
                        <CheckCircle2 className="h-3.5 w-3.5" />
                      </button>
                      <div className="min-w-0 flex-1">
                        <div className="mb-1 flex flex-wrap items-center gap-2">
                          <span className="rounded-full border border-[#373737] px-1.5 py-0.5 text-[10px] uppercase text-[#d4a574]">{result.provider}</span>
                          {result.planned_kind && (
                            <span className="rounded-full border border-[#373737] px-1.5 py-0.5 text-[10px] uppercase text-[#9fb7ff]">{result.planned_kind}</span>
                          )}
                          {typeof result.rank === "number" && (
                            <span className="rounded-full border border-[#373737] px-1.5 py-0.5 text-[10px] uppercase text-[#9bd6b5]">#{result.rank}</span>
                          )}
                          <span className="text-[10px] uppercase text-[#787878]">{result.category}</span>
                        </div>
                        <h3 className="break-words text-sm font-medium text-[#e8e8e8]">{result.title || "Untitled"}</h3>
                        {result.url && (
                          <button
                            type="button"
                            onClick={() => openUrl(result.url)}
                            title={result.url}
                            className="mt-1 block w-full truncate text-left text-xs text-[#787878] underline-offset-2 hover:text-[#d4a574] hover:underline"
                          >
                            {result.url}
                          </button>
                        )}
                        {result.reason && <p className="mt-1 text-[11px] leading-4 text-[#ebc396]">{result.reason}</p>}
                        {result.planned_query && result.planned_query !== query.trim() && (
                          <div className="mt-1 truncate text-[11px] text-[#787878]">Task: {result.planned_query}</div>
                        )}
                        <p className="mt-2 line-clamp-4 text-xs leading-5 text-[#b4b4b4]">{result.content}</p>
                        <div className="mt-2 flex items-center justify-end gap-2">
                          {result.url && <CopyUrlButton url={result.url} />}
                          <button
                            type="button"
                            onClick={() => sendResultToArena(result)}
                            disabled={!canSave || arenaSendingUrl === result.url}
                            title={canSave ? "Send to Arena as a card" : "files.write and ai.write permissions required"}
                            className={cn(
                              "inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-[11px] transition-colors",
                              canSave
                                ? "border-[#d4a574]/40 text-[#ebc396] hover:border-[#d4a574] hover:bg-[#2d241a]"
                                : "border-[#373737] text-[#5f5f5f] cursor-not-allowed",
                            )}
                          >
                            {arenaSendingUrl === result.url ? <Loader2 className="h-3 w-3 animate-spin" /> : <Network className="h-3 w-3" />}
                            Send to Arena
                          </button>
                        </div>
                      </div>
                    </div>
                  </article>
                ))}
              </div>

              <aside className="space-y-3">
                <div className="rounded-md border border-[#373737] bg-[#1a1a1a] p-3">
                  <div className="text-xs font-medium text-[#e8e8e8]">Selected</div>
                  <div className="mt-1 text-2xl font-semibold text-[#ebc396]">{selectedResults.length}</div>
                  <p className="mt-2 text-xs leading-5 text-[#787878]">Save runs AI extraction, then creates a Markdown note and BibTeX file in the Research folder.</p>
                  {!canSave && (
                    <p className="mt-2 rounded-md border border-[#5f3f24] bg-[#2d241a] px-2 py-1.5 text-xs text-[#ebc396]">files.write and ai.write permissions are required.</p>
                  )}
                  <Button onClick={saveSelected} disabled={saving || selectedResults.length === 0 || !canSave} className="mt-3 w-full bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]">
                    {saving ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <Save className="mr-2 h-4 w-4" />}
                    Save & Extract
                  </Button>
                </div>

                <div className="rounded-md border border-[#373737] bg-[#1a1a1a] p-3">
                  <div className="mb-2 flex items-center justify-between">
                    <div className="text-xs font-medium text-[#e8e8e8]">Recent References</div>
                    <button onClick={() => listResearchItems(projectId).then(setItems).catch(() => setItems([]))} className="text-[11px] text-[#d4a574]">Refresh</button>
                  </div>
                  <div className="space-y-2">
                    {items.length === 0 ? (
                      <div className="py-4 text-center text-xs text-[#787878]">No references saved.</div>
                    ) : items.slice(0, 8).map((item) => (
                      <article key={item.id} className="rounded border border-[#2a2a2a] bg-[#111111] p-2">
                        <div className="text-[10px] uppercase text-[#d4a574]">{item.category}</div>
                        <h4 className="mt-1 line-clamp-2 text-xs font-medium text-[#e8e8e8]">{item.title || "Untitled"}</h4>
                        {item.summary && <p className="mt-1 line-clamp-2 text-[11px] leading-4 text-[#787878]">{item.summary}</p>}
                        <div className="mt-1.5 flex items-center justify-end gap-1.5">
                          {item.url && <CopyUrlButton url={item.url} size="xs" />}
                          <button
                            type="button"
                            onClick={() => sendItemToArena(item)}
                            disabled={!canSave || arenaSendingUrl === item.id}
                            title={canSave ? "Send to Arena as a card" : "files.write and ai.write permissions required"}
                            className={cn(
                              "inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-[10px] transition-colors",
                              canSave
                                ? "border-[#d4a574]/40 text-[#ebc396] hover:border-[#d4a574] hover:bg-[#2d241a]"
                                : "border-[#373737] text-[#5f5f5f] cursor-not-allowed",
                            )}
                          >
                            {arenaSendingUrl === item.id ? <Loader2 className="h-2.5 w-2.5 animate-spin" /> : <Network className="h-2.5 w-2.5" />}
                            Arena
                          </button>
                        </div>
                      </article>
                    ))}
                  </div>
                </div>
              </aside>
            </div>
          </ScrollArea>
        </div>
      )}
      {message && mode === "agent" && (
        <div className="border-t border-[#373737] bg-[#121212] px-3 py-2 text-xs text-[#b4b4b4]">{message}</div>
      )}
    </div>
  )
}

const DEEPSEEK_MODELS = [
  { value: "deepseek-v4-pro", label: "V4 Pro", desc: "Deep reasoning, 32K context" },
  { value: "deepseek-v4-flash", label: "V4 Flash", desc: "Fast responses, 32K context" },
  { value: "deepseek-chat", label: "V3 Chat", desc: "General purpose, 64K context" },
]

const ROLE_OPTIONS: ProjectRole[] = ["owner", "editor", "viewer"]
// Ownership is singular and non-transferable on the server, so it's never an
// assignable choice in invite/promote dropdowns — only "editor"/"viewer" are.
const ASSIGNABLE_ROLE_OPTIONS: ProjectRole[] = ["editor", "viewer"]

function capabilityLabel(capability: ProjectCapability) {
  return capability.replace(".", " ")
}

function ScreenPreview({ stream, onStop }: { stream: MediaStream; onStop: () => void }) {
  const videoRef = useRef<HTMLVideoElement | null>(null)

  useEffect(() => {
    if (videoRef.current) videoRef.current.srcObject = stream
  }, [stream])

  return (
    <div className="mt-3 overflow-hidden rounded-md border border-[#373737] bg-[#0d0d0d]">
      <div className="flex h-8 items-center justify-between border-b border-[#373737] px-2">
        <span className="text-[11px] text-[#b4b4b4]">Live app screen</span>
        <button onClick={onStop} className="text-[11px] text-[#ffb4a8] hover:text-[#ffd1ca]">Stop watching</button>
      </div>
      <video ref={videoRef} autoPlay playsInline muted className="aspect-video w-full bg-black object-contain" />
    </div>
  )
}

function MembersPanel({
  projectId,
  project,
  capabilities,
  currentUserId,
  screenShare,
  onProjectRefresh,
}: {
  projectId: string
  project: Project | null
  capabilities: ProjectCapability[]
  currentUserId: string | null
  screenShare: ReturnType<typeof useScreenShare>
  onProjectRefresh: () => Promise<void>
}) {
  const [members, setMembers] = useState<ProjectMember[]>([])
  const [invites, setInvites] = useState<ProjectInvite[]>([])
  const [invite, setInvite] = useState("")
  const [inviteRole, setInviteRole] = useState<ProjectRole>("editor")
  const canManageMembers = capabilities.includes("members.manage")
  const canManageInvites = capabilities.includes("invites.manage")
  const showShareControl = screenShare.canShare && project?.role !== "owner"
  const showViewControl = screenShare.canView

  const refreshMembers = async () => {
    try { setMembers(await listProjectMembers(projectId)) } catch { setMembers([]) }
  }

  const refreshInvites = async () => {
    if (!canManageInvites) return
    try { setInvites(await listProjectInvites(projectId)) } catch { setInvites([]) }
  }

  useEffect(() => {
    void refreshMembers()
    void refreshInvites()
  }, [projectId, canManageInvites])

  const createInvite = async () => {
    if (!canManageInvites) return
    const result = await createProjectInvite(projectId, { role: inviteRole })
    setInvite(result.code)
    await refreshInvites()
  }

  const copyInvite = async (code = invite) => {
    if (!code) return
    await navigator.clipboard?.writeText(code)
  }

  const revokeInvites = async () => {
    if (!canManageInvites) return
    await revokeProjectInvite(projectId)
    setInvite("")
    await refreshInvites()
  }

  const resetInvite = async () => {
    await revokeInvites()
    await createInvite()
  }

  const removeMember = async (member: ProjectMember) => {
    if (!canManageMembers || member.user_id === currentUserId) return
    await removeProjectMember(projectId, member.user_id)
    await refreshMembers()
    await onProjectRefresh()
  }

  const updateMember = async (
    member: ProjectMember,
    role: ProjectRole,
    nextCapabilities = parseCapabilities(member.capabilities),
  ) => {
    if (!canManageMembers) return
    await updateProjectMember(projectId, member.user_id, { role, capabilities: nextCapabilities })
    await refreshMembers()
    await onProjectRefresh()
  }

  const toggleCapability = async (member: ProjectMember, capability: ProjectCapability) => {
    const current = parseCapabilities(member.capabilities)
    const next = current.includes(capability)
      ? current.filter((cap) => cap !== capability)
      : [...current, capability]
    await updateMember(member, member.role, next)
  }

  return (
    <div className="rounded-lg border border-[#373737] bg-[#1a1a1a] p-4">
      <div className="mb-3">
        <h3 className="text-sm font-medium text-[#e8e8e8]">Members</h3>
        <p className="mt-1 text-xs text-[#787878]">
          Your role: {project?.role ?? "unknown"}
        </p>
      </div>

      {canManageInvites && (
        <div className="mb-4 rounded-md border border-[#373737] bg-[#121212] p-3">
          <div className="mb-2 flex items-center gap-2">
            <select
              value={inviteRole}
              onChange={(event) => setInviteRole(event.target.value as ProjectRole)}
              className="rounded-md border border-[#373737] bg-[#232323] px-2 py-1 text-xs text-[#b4b4b4]"
            >
              {ASSIGNABLE_ROLE_OPTIONS.map((role) => <option key={role} value={role}>{role}</option>)}
            </select>
            <Button onClick={createInvite} size="sm" className="h-7 bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]">
              Create invite
            </Button>
            <Button onClick={resetInvite} size="icon" variant="ghost" className="h-7 w-7" title="Reset invite">
              <RotateCcw className="h-3.5 w-3.5" />
            </Button>
            <Button onClick={revokeInvites} size="icon" variant="ghost" className="h-7 w-7" title="Revoke invites">
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </div>
          {invite && (
            <div className="flex items-center gap-2">
              <p className="min-w-0 flex-1 truncate font-mono text-xs text-[#d4a574]">Invite code: {invite}</p>
              <Button onClick={() => copyInvite()} size="icon" variant="ghost" className="h-7 w-7" title="Copy invite">
                <Copy className="h-3.5 w-3.5" />
              </Button>
            </div>
          )}
          {invites.length > 0 && (
            <div className="mt-2 space-y-1">
              {invites.slice(0, 3).map((item) => (
                <div key={item.id} className="flex items-center gap-2 text-[11px] text-[#787878]">
                  <span className="font-mono text-[#b4b4b4]">{item.code}</span>
                  <span>{item.role}</span>
                  <span>{item.used_count}/{item.max_uses}</span>
                  <button onClick={() => copyInvite(item.code)} className="ml-auto text-[#d4a574] hover:text-[#ebc396]">copy</button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {(showShareControl || showViewControl) && (
        <div className="mb-4 rounded-md border border-[#373737] bg-[#121212] p-3">
          <div className="flex items-center gap-2">
            {showShareControl && (
              <Button
                onClick={screenShare.sharing ? screenShare.stopShare : screenShare.startShare}
                size="sm"
                className={cn(
                  "h-7",
                  screenShare.sharing ? "bg-[#5f2424] text-[#ffb4a8] hover:bg-[#733030]" : "bg-[#232323] text-[#b4b4b4] hover:bg-[#2d2d2d]",
                )}
              >
                {screenShare.sharing ? <MonitorX className="mr-1.5 h-3.5 w-3.5" /> : <MonitorUp className="mr-1.5 h-3.5 w-3.5" />}
                {screenShare.sharing ? "Stop sharing" : "Share app screen"}
              </Button>
            )}
            {showViewControl && !showShareControl && (
              <span className="text-xs text-[#b4b4b4]">
                {screenShare.remoteStream ? "Watching teammate screen" : "Waiting for teammate screen share"}
              </span>
            )}
            <span className="ml-auto text-[11px] text-[#787878]">{screenShare.connected ? "screen relay connected" : "screen relay offline"}</span>
          </div>
          {screenShare.error && <p className="mt-2 text-[11px] text-[#ffb4a8]">{screenShare.error}</p>}
          {showViewControl && screenShare.remoteStream && (
            <ScreenPreview stream={screenShare.remoteStream} onStop={screenShare.stopWatching} />
          )}
        </div>
      )}

      <div className="space-y-2">
        {members.map((member) => {
          const memberCaps = parseCapabilities(member.capabilities)
          return (
            <div key={member.user_id} className="rounded-md border border-[#373737] bg-[#121212] p-3">
              <div className="flex items-center gap-2">
                <div className="min-w-0 flex-1">
                  <p className="truncate text-xs font-medium text-[#e8e8e8]">{member.display_name || member.email}</p>
                  <p className="truncate text-[11px] text-[#787878]">{member.email}</p>
                </div>
                <select
                  value={member.role}
                  disabled={!canManageMembers || member.user_id === currentUserId || member.role === "owner"}
                  onChange={(event) => updateMember(member, event.target.value as ProjectRole)}
                  className="rounded-md border border-[#373737] bg-[#232323] px-2 py-1 text-xs text-[#b4b4b4] disabled:opacity-60"
                >
                  {/* Owners render a locked "owner" option (ownership is not
                      transferable); everyone else can only be editor/viewer. */}
                  {(member.role === "owner" ? ROLE_OPTIONS : ASSIGNABLE_ROLE_OPTIONS).map((role) => (
                    <option key={role} value={role}>{role}</option>
                  ))}
                </select>
                {canManageMembers && member.user_id !== currentUserId && (
                  <Button onClick={() => removeMember(member)} size="icon" variant="ghost" className="h-7 w-7" title="Remove member">
                    <Trash2 className="h-3.5 w-3.5 text-[#ff8a80]" />
                  </Button>
                )}
              </div>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {ALL_PROJECT_CAPABILITIES.map((capability) => (
                  <button
                    key={capability}
                    disabled={!canManageMembers}
                    onClick={() => toggleCapability(member, capability)}
                    className={cn(
                      "rounded-full border px-2 py-0.5 text-[10px] transition-colors disabled:cursor-not-allowed",
                      memberCaps.includes(capability)
                        ? "border-[#d4a574] bg-[#2d241a] text-[#ebc396]"
                        : "border-[#373737] bg-[#1a1a1a] text-[#787878]",
                    )}
                  >
                    {capabilityLabel(capability)}
                  </button>
                ))}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function SettingsPanel({
  projectId,
  project,
  capabilities,
  currentUserId,
  screenShare,
  onProjectRefresh,
}: {
  projectId: string
  project: Project | null
  capabilities: ProjectCapability[]
  currentUserId: string | null
  screenShare: ReturnType<typeof useScreenShare>
  onProjectRefresh: () => Promise<void>
}) {
  const [status, setStatus] = useState<AiConfigStatus | null>(null)
  const [sidecarRunning, setSidecarRunning] = useState<boolean | null>(null)
  const [apiKey, setApiKey] = useState("")
  const [firecrawlKey, setFirecrawlKey] = useState("")
  const [context7Key, setContext7Key] = useState("")
  const [tavilyKey, setTavilyKey] = useState("")
  const [searxngUrl, setSearxngUrl] = useState("http://localhost:8080")
  const [model, setModel] = useState("deepseek-v4-pro")

  useEffect(() => {
    getAiConfigStatus().then((value) => {
      setStatus(value)
      setModel(value.model)
      setSearxngUrl(value.searxng_url || "http://localhost:8080")
    }).catch(() => {})
    getSidecarStatus().then(setSidecarRunning).catch(() => setSidecarRunning(false))
  }, [])

  const save = async () => {
    await setAiConfig({
      api_key: apiKey || null,
      base_url: status?.base_url || "https://api.deepseek.com",
      model,
      firecrawl_api_key: firecrawlKey || null,
      context7_api_key: context7Key || null,
      tavily_api_key: tavilyKey || null,
      searxng_url: searxngUrl || "http://localhost:8080",
    })
    setStatus(await getAiConfigStatus())
    setApiKey("")
    setFirecrawlKey("")
    setTavilyKey("")
  }

  return (
    <div className="h-full overflow-auto bg-[#0d0d0d] p-4">
      <div className="mx-auto grid max-w-5xl gap-4 lg:grid-cols-[380px_1fr]">
        <div className="space-y-5 rounded-lg border border-[#373737] bg-[#1a1a1a] p-6">
        <div>
          <h3 className="text-sm font-medium text-[#e8e8e8]">Model Provider</h3>
          <p className="mt-1 text-xs text-[#787878]">DeepSeek &middot; api.deepseek.com/anthropic</p>
        </div>

        <div className="space-y-2">
          <label className="text-xs font-medium text-[#b4b4b4]">API Key</label>
          <Input
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            placeholder="sk-xxxxxxxxxxxxxxxx"
            className="border-[#373737] bg-[#232323] text-sm"
            type="password"
          />
          <p className="text-xs text-[#787878]">
            Get your key at{" "}
            <a href="https://platform.deepseek.com/api_keys" target="_blank" rel="noreferrer" className="text-[#d4a574] underline">
              platform.deepseek.com
            </a>
          </p>
        </div>

        <div className="space-y-2">
          <label className="text-xs font-medium text-[#b4b4b4]">Model</label>
          <div className="space-y-1.5">
            {DEEPSEEK_MODELS.map((m) => (
              <button
                key={m.value}
                onClick={() => setModel(m.value)}
                className={cn(
                  "w-full rounded-md border px-3 py-2 text-left",
                  model === m.value
                    ? "border-[#d4a574] bg-[#2d2d2d] text-[#e8e8e8]"
                    : "border-[#373737] bg-[#232323] text-[#b4b4b4] hover:border-[#464646]",
                )}
              >
                <div className="text-sm font-medium">{m.label}</div>
                <div className="text-xs text-[#787878]">{m.desc}</div>
              </button>
            ))}
          </div>
        </div>

        <div className="space-y-3 border-t border-[#373737] pt-4">
          <div>
            <h3 className="text-sm font-medium text-[#e8e8e8]">Research Providers</h3>
            <p className="mt-1 text-xs text-[#787878]">Firecrawl powers web search. Context7 powers docs search. Tavily powers the /search AI search page.</p>
          </div>
          <div className="space-y-2">
            <label className="text-xs font-medium text-[#b4b4b4]">Firecrawl API Key</label>
            <Input
              value={firecrawlKey}
              onChange={(event) => setFirecrawlKey(event.target.value)}
              placeholder={status?.firecrawl_configured ? "Configured" : "fc-xxxxxxxxxxxxxxxx"}
              className="border-[#373737] bg-[#232323] text-sm"
              type="password"
            />
          </div>
          <div className="space-y-2">
            <label className="text-xs font-medium text-[#b4b4b4]">Context7 API Key</label>
            <Input
              value={context7Key}
              onChange={(event) => setContext7Key(event.target.value)}
              placeholder={status?.context7_configured ? "Configured" : "Optional"}
              className="border-[#373737] bg-[#232323] text-sm"
              type="password"
            />
          </div>
          <div className="space-y-2">
            <label className="text-xs font-medium text-[#b4b4b4]">Tavily API Key</label>
            <Input
              value={tavilyKey}
              onChange={(event) => setTavilyKey(event.target.value)}
              placeholder={status?.tavily_configured ? "Configured" : "tvly-xxxxxxxxxxxxxxxx"}
              className="border-[#373737] bg-[#232323] text-sm"
              type="password"
            />
          </div>
          <div className="space-y-2 border-t border-[#373737] pt-3">
            <label className="text-xs font-medium text-[#b4b4b4]">SearXNG URL · Chat fallback</label>
            <Input
              value={searxngUrl}
              onChange={(event) => setSearxngUrl(event.target.value)}
              className="border-[#373737] bg-[#232323] text-sm"
            />
            <p className="text-xs leading-5 text-[#787878]">
              Research Search uses Firecrawl and Context7. Tavily powers the /search AI search page. SearXNG is only used by chat.
            </p>
          </div>

          <div className="space-y-2 border-t border-[#373737] pt-3">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-[#b4b4b4]">Academic Search Engine</label>
              {sidecarRunning === null ? (
                <span className="text-xs text-[#787878]">Checking…</span>
              ) : sidecarRunning ? (
                <span className="flex items-center gap-1.5 text-xs text-[#9bd6b5]">
                  <span className="h-1.5 w-1.5 rounded-full bg-[#9bd6b5]" />
                  Running
                </span>
              ) : (
                <span className="flex items-center gap-1.5 text-xs text-[#d49a9a]">
                  <span className="h-1.5 w-1.5 rounded-full bg-[#d49a9a]" />
                  {status?.sidecar_enabled === false ? "Disabled" : "Not running"}
                </span>
              )}
            </div>
            <p className="text-xs leading-5 text-[#787878]">
              Paper and dataset searches query arXiv, Semantic Scholar, OpenAlex, Zenodo, and Kaggle directly via a Python sidecar. Requires Python with{" "}
              <code className="text-[#b4b4b4]">fastapi uvicorn httpx</code> installed. Falls back to Firecrawl/Tavily when unavailable.
            </p>
          </div>
        </div>

        <Button
          onClick={save}
          className="w-full bg-[#d4a574] text-[#111111] hover:bg-[#ebc396]"
        >
          Save AI Settings
        </Button>
        </div>

        <MembersPanel
          projectId={projectId}
          project={project}
          capabilities={capabilities}
          currentUserId={currentUserId}
          screenShare={screenShare}
          onProjectRefresh={onProjectRefresh}
        />
      </div>
    </div>
  )
}

export function ModelerWorkbench({ projectId }: { projectId: string }) {
  const tauriAgent = useTauriAgent()
  const router = useRouter()
  const { logout, user } = useAuth()
  const localWriteTimers = useRef<Record<string, ReturnType<typeof setTimeout>>>({})
  const restoredHostFolderRef = useRef<string | null>(null)
  const [activeActivity, setActiveActivity] = useState<Activity>("explorer")
  const [activeTab, setActiveTab] = useState("chat")
  const [tabs, setTabs] = useState<Tab[]>([
    { id: "chat", title: "Chat", kind: "chat" },
  ])
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [sessionQuery, setSessionQuery] = useState("")
  const [showArchived, setShowArchived] = useState(false)

  // On mount, populate the session list. Without this the sidebar stays empty
  // until the user clicks "+ New Chat" (which is the only call site that
  // triggered refreshSessions() before this PR). Safe under StrictMode —
  // refreshSessions is idempotent.
  useEffect(() => {
    refreshSessions()
  }, [])

  // Sync tab titles from server-side session names. Sessions auto-title from
  // the first user message (see session.rs:144-151), but tabs are created with
  // a placeholder name at switchToChat() time. After refreshSessions populates
  // `sessions`, propagate the canonical name back into matching chat tabs.
  // One-way server → tab; user renames flow through rename_session → refreshSessions.
  useEffect(() => {
    setTabs((prev) =>
      prev.map((tab) => {
        if (tab.kind !== "chat") return tab
        const session = sessions.find((s) => s.id === tab.id)
        if (!session) return tab
        const next = session.name?.trim() || "New Chat"
        if (tab.title === next) return tab
        return { ...tab, title: next }
      }),
    )
  }, [sessions])
  const [workspaceMode, setWorkspaceMode] = useState<WorkspaceMode>("host")
  const [remoteTree, setRemoteTree] = useState<FileTreeItem | null>(null)
  const [remoteStatus, setRemoteStatus] = useState<"idle" | "loading" | "error">("idle")
  const [syncing, setSyncing] = useState(false)
  const [syncStats, setSyncStats] = useState<SyncStats | null>(null)
  // Surfaces failures from conflict-resolution actions (keep/overwrite/diff),
  // which hit the network and would otherwise throw unhandled promise
  // rejections ("Failed to fetch") with no user-visible feedback.
  const [conflictError, setConflictError] = useState<string | null>(null)
  // LaTeX compile feedback, keyed by the .tex tab id so each tab tracks its own
  // run state and last error independently.
  const [latexCompiling, setLatexCompiling] = useState<string | null>(null)
  const [latexError, setLatexError] = useState<{ path: string; log: string } | null>(null)
  const [pendingAutoSync, setPendingAutoSync] = useState(false)
  const [autoSyncEnabled, setAutoSyncEnabled] = useState(false)
  const [hostFolderMissing, setHostFolderMissing] = useState(false)
  const [lastAutoSyncSignature, setLastAutoSyncSignature] = useState("")
  const [project, setProject] = useState<Project | null>(null)
  const capabilities = useMemo(() => parseCapabilities(project?.capabilities), [project])
  const screenShare = useScreenShare(projectId, capabilities)
  const currentUserId = user?.id || decodeCurrentUserId() || (project?.role === "owner" ? project.owner_id : null)
  const canSyncWorkspace = capabilities.includes("workspace.sync") && capabilities.includes("files.write")
  const canWriteFiles = capabilities.includes("files.write")
  const localTreeSignature = useMemo(
    () => tauriAgent.fileTree ? JSON.stringify({
      tree: tauriAgent.fileTree,
      contents: Object.fromEntries(
        Object.entries(tauriAgent.fileContents)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([path, content]) => [path, hashContent(content)]),
      ),
    }) : "",
    [tauriAgent.fileTree, tauriAgent.fileContents],
  )

  useEffect(() => {
    tauriAgent.connect()
  }, [tauriAgent.connect])

  useEffect(() => {
    return () => {
      Object.values(localWriteTimers.current).forEach((timer) => clearTimeout(timer))
    }
  }, [])

  const refreshProject = async () => {
    try { setProject(await getProject(projectId)) } catch { setProject(null) }
  }

  useEffect(() => {
    void refreshProject()
  }, [projectId])

  useEffect(() => {
    const stored = localStorage.getItem(workspaceModeKey(projectId, user?.id ?? ""))
    if (stored === "host" || stored === "guest") setWorkspaceMode(stored)
  }, [projectId])

  useEffect(() => {
    localStorage.setItem(workspaceModeKey(projectId, user?.id ?? ""), workspaceMode)
  }, [projectId, workspaceMode])

  useEffect(() => {
    if (typeof window === "undefined") return
    setHostFolderMissing(false)
    const enabled = localStorage.getItem(autoSyncKey(projectId, user?.id ?? "")) === "true"
    setAutoSyncEnabled(enabled)
  }, [projectId])

  useEffect(() => {
    if (!canSyncWorkspace || workspaceMode !== "host" || !autoSyncEnabled) return
    const storedPath = localStorage.getItem(hostFolderKey(projectId, user?.id ?? ""))
    if (!storedPath || restoredHostFolderRef.current === `${projectId}:${storedPath}`) return
    restoredHostFolderRef.current = `${projectId}:${storedPath}`
    tauriAgent.changeDir(storedPath)
      .then(() => {
        setHostFolderMissing(false)
        setLastAutoSyncSignature("")
      })
      .catch(() => {
        setHostFolderMissing(true)
        setAutoSyncEnabled(false)
      })
  }, [autoSyncEnabled, canSyncWorkspace, projectId, tauriAgent, workspaceMode])

  useEffect(() => {
    if (typeof window === "undefined") return
    localStorage.setItem(autoSyncKey(projectId, user?.id ?? ""), autoSyncEnabled ? "true" : "false")
  }, [autoSyncEnabled, projectId])

  const refreshRemoteTree = async () => {
    setRemoteStatus("loading")
    try {
      const items = await listProjectTree(projectId)
      setRemoteTree(remoteTreeToFileTree(items))
      setRemoteStatus("idle")
    } catch {
      setRemoteTree(null)
      setRemoteStatus("error")
    }
  }

  const signOut = () => {
    logout()
    router.push("/login")
  }

  const syncHostToRemote = async (sourceTree = tauriAgent.fileTree) => {
    if (!sourceTree || syncing || !canSyncWorkspace) return

    const stats = emptySyncStats()
    setSyncing(true)
    setSyncStats(null)

    try {
      const remoteItems = await listProjectTree(projectId)
      const manifest = loadSyncManifest(projectId, user?.id ?? "")
      const nextManifest: SyncManifest = { ...manifest }
      const desiredRemoteIds = new Set<string>()
      const reservedRootNames = new Set(["Code", "Paper", "Research"])

      let codeRoot = findRemoteChild(remoteItems, "Code", "folder")
      if (!codeRoot) {
        const created = await createProjectFile(projectId, {
          name: "Code",
          type: "folder",
          parent_id: null,
          zone: "code",
        })
        codeRoot = {
          id: created.id,
          name: created.name,
          type: "folder",
          zone: created.zone,
          updated_at: created.updated_at,
          children: [],
        }
        remoteItems.push(codeRoot)
        stats.created += 1
      }

      const syncNodes = async (
        localNodes: FileTreeItem[],
        remoteNodes: ProjectFileTreeItem[],
        parentId: string | null,
      ) => {
        const mirror = [...remoteNodes]

        for (const localNode of localNodes) {
          if (localNode.type === "folder") {
            const conflict = mirror.find((node) => node.name === localNode.name && node.type !== "folder")
            if (conflict) {
              stats.skipped += 1
              continue
            }

            let remoteFolder = findRemoteChild(mirror, localNode.name, "folder")
            if (!remoteFolder) {
              try {
                const created = await createProjectFile(projectId, {
                  name: localNode.name,
                  type: "folder",
                  parent_id: parentId,
                  zone: "code",
                })
                remoteFolder = {
                  id: created.id,
                  name: created.name,
                  type: "folder",
                  zone: created.zone,
                  updated_at: created.updated_at,
                  children: [],
                }
                mirror.push(remoteFolder)
                stats.created += 1
              } catch {
                stats.failed += 1
                continue
              }
            }

            desiredRemoteIds.add(remoteFolder.id)
            remoteFolder.children = await syncNodes(
              localNode.children ?? [],
              remoteFolder.children ?? [],
              remoteFolder.id,
            )
            continue
          }

          const content = await tauriAgent.openFile(localNode.path)
          if (content == null) {
            stats.skipped += 1
            continue
          }
          const localHash = hashContent(content)
          const pathKey = localNode.path.replace(/\\/g, "/")

          const conflict = mirror.find((node) => node.name === localNode.name && node.type !== "file")
          if (conflict) {
            stats.skipped += 1
            continue
          }

          let remoteFile = findRemoteChild(mirror, localNode.name, "file")
          let createdFile = false
          if (!remoteFile) {
            try {
              const created = await createProjectFile(projectId, {
                name: localNode.name,
                type: "file",
                parent_id: parentId,
                zone: "code",
              })
              remoteFile = {
                id: created.id,
                name: created.name,
                type: "file",
                zone: created.zone,
                updated_at: created.updated_at,
              }
              mirror.push(remoteFile)
              createdFile = true
            } catch {
              stats.failed += 1
              continue
            }
          }

          desiredRemoteIds.add(remoteFile.id)
          try {
            if (!createdFile) {
              const previous = manifest[pathKey]
              if (previous?.fileId === remoteFile.id) {
                let currentRemoteHash = previous.remoteHash
                let currentRemoteUpdatedAt = previous.remoteUpdatedAt
                const knownDiverged = previous.localHash !== previous.remoteHash
                const localChanged = localHash !== previous.localHash
                if (previous.remoteUpdatedAt !== remoteVersion(remoteFile) || knownDiverged || localChanged) {
                  const remoteContent = await getProjectFileContent(projectId, remoteFile.id)
                  currentRemoteHash = hashContent(remoteContent.content)
                  currentRemoteUpdatedAt = remoteContent.updated_at
                  remoteFile.updated_at = remoteContent.updated_at
                }

                const remoteChanged = currentRemoteHash !== previous.remoteHash
                if (!localChanged && !remoteChanged) {
                  stats.skipped += 1
                  continue
                }
                if (!localChanged && remoteChanged) {
                  nextManifest[pathKey] = {
                    fileId: remoteFile.id,
                    remoteUpdatedAt: currentRemoteUpdatedAt,
                    localHash: previous.localHash,
                    remoteHash: currentRemoteHash,
                  }
                  stats.skipped += 1
                  continue
                }
                if (localChanged && (remoteChanged || knownDiverged)) {
                  if (localHash === currentRemoteHash) {
                    nextManifest[pathKey] = {
                      fileId: remoteFile.id,
                      remoteUpdatedAt: currentRemoteUpdatedAt,
                      localHash,
                      remoteHash: currentRemoteHash,
                    }
                    stats.skipped += 1
                    continue
                  }
                  addSyncConflict(stats, {
                    path: pathKey,
                    fileId: remoteFile.id,
                    remoteUpdatedAt: currentRemoteUpdatedAt,
                  })
                  continue
                }
              } else {
                const remoteContent = await getProjectFileContent(projectId, remoteFile.id)
                const remoteHash = hashContent(remoteContent.content)
                if (remoteHash !== localHash) {
                  addSyncConflict(stats, {
                    path: pathKey,
                    fileId: remoteFile.id,
                    remoteUpdatedAt: remoteContent.updated_at,
                  })
                  continue
                }
                nextManifest[pathKey] = {
                  fileId: remoteFile.id,
                  remoteUpdatedAt: remoteContent.updated_at,
                  localHash,
                  remoteHash,
                }
                stats.skipped += 1
                continue
              }
            }

            const saved = await updateProjectFileContent(
              projectId,
              remoteFile.id,
              content,
              createdFile ? undefined : remoteVersion(remoteFile),
            )
            nextManifest[pathKey] = {
              fileId: remoteFile.id,
              remoteUpdatedAt: saved.updated_at,
              localHash,
              remoteHash: localHash,
            }
            if (createdFile) stats.created += 1
            else stats.updated += 1
          } catch {
            stats.failed += 1
          }
        }

        return mirror
      }

      codeRoot.children = await syncNodes(sourceTree.children ?? [], codeRoot.children ?? [], codeRoot.id)

      const pruneRemoteNodes = async (nodes: ProjectFileTreeItem[], parentPath = "") => {
        for (const node of nodes) {
          if (node.zone !== "code") continue
          const pathKey = parentPath ? `${parentPath}/${node.name}` : node.name
          if (desiredRemoteIds.has(node.id)) {
            if (node.children?.length) await pruneRemoteNodes(node.children, pathKey)
            continue
          }
          if (node.type !== "file") {
            if (node.children?.length) await pruneRemoteNodes(node.children, pathKey)
            continue
          }
          const previous = manifest[pathKey]
          if (!previous || previous.fileId !== node.id) {
            stats.skipped += 1
            continue
          }
          try {
            const remoteContent = await getProjectFileContent(projectId, node.id)
            const remoteHash = hashContent(remoteContent.content)
            if (previous.localDeleted) {
              nextManifest[pathKey] = {
                ...previous,
                remoteUpdatedAt: remoteContent.updated_at,
                remoteHash,
              }
              stats.skipped += 1
              continue
            }
            if (remoteHash !== previous.remoteHash || previous.localHash !== previous.remoteHash) {
              addSyncConflict(stats, {
                path: pathKey,
                fileId: node.id,
                remoteUpdatedAt: remoteContent.updated_at,
                localDeleted: true,
              })
              continue
            }
            await deleteProjectFile(projectId, node.id)
            delete nextManifest[pathKey]
            stats.deleted += 1
          } catch {
            stats.failed += 1
          }
        }
      }

      await pruneRemoteNodes(codeRoot.children ?? [])
      for (const node of remoteItems) {
        if (node.zone !== "code" || reservedRootNames.has(node.name)) continue
        try {
          await deleteProjectFile(projectId, node.id)
          stats.deleted += 1
        } catch {
          stats.failed += 1
        }
      }
      saveSyncManifest(projectId, user?.id ?? "", nextManifest)
      setSyncStats(stats)
      await refreshRemoteTree()
    } catch {
      stats.failed += 1
      setSyncStats(stats)
    } finally {
      setSyncing(false)
    }
  }

  useEffect(() => {
    // The remote tree is needed in BOTH modes now: guest renders it directly,
    // and host grafts the cloud-only Paper/Research zones onto the local tree
    // (see `hostTree`) so saved references/papers are visible while working
    // against the local folder.
    void refreshRemoteTree()
  }, [workspaceMode, projectId])

  useEffect(() => {
    if (!pendingAutoSync || !tauriAgent.fileTree || syncing) return
    setPendingAutoSync(false)
    setAutoSyncEnabled(true)
    void syncHostToRemote(tauriAgent.fileTree).then(() => setLastAutoSyncSignature(localTreeSignature))
  }, [pendingAutoSync, tauriAgent.fileTree, syncing, localTreeSignature])

  useEffect(() => {
    if (!autoSyncEnabled || workspaceMode !== "host" || !canSyncWorkspace || !tauriAgent.fileTree || syncing) return
    if (!localTreeSignature || localTreeSignature === lastAutoSyncSignature) return

    const timer = window.setTimeout(() => {
      void syncHostToRemote(tauriAgent.fileTree).then(() => setLastAutoSyncSignature(localTreeSignature))
    }, 1500)

    return () => window.clearTimeout(timer)
  }, [
    autoSyncEnabled,
    workspaceMode,
    canSyncWorkspace,
    tauriAgent.fileTree,
    syncing,
    localTreeSignature,
    lastAutoSyncSignature,
  ])

  const openFolderAndSync = async () => {
    if (!canSyncWorkspace) return
    setSyncStats(null)
    const path = await tauriAgent.openFolder()
    if (path) {
      localStorage.setItem(hostFolderKey(projectId, user?.id ?? ""), path)
      localStorage.setItem(autoSyncKey(projectId, user?.id ?? ""), "true")
      setHostFolderMissing(false)
    }
    setPendingAutoSync(Boolean(path))
  }

  const syncCurrentHostTree = async () => {
    if (!tauriAgent.fileTree || syncing || !canSyncWorkspace) return
    setAutoSyncEnabled(true)
    localStorage.setItem(autoSyncKey(projectId, user?.id ?? ""), "true")
    if (tauriAgent.workDir) localStorage.setItem(hostFolderKey(projectId, user?.id ?? ""), tauriAgent.workDir)
    await syncHostToRemote(tauriAgent.fileTree)
    setLastAutoSyncSignature(localTreeSignature)
  }

  const refreshSessions = async () => {
    try {
      setSessionQuery("")
      setSessions(await listSessions(user?.id ?? ""))
    } catch {}
  }

  const handleSessionSearch = async (query: string) => {
    setSessionQuery(query)
    try {
      setSessions(
        query.trim()
          ? await searchSessions(user?.id ?? "", query)
          : await listSessions(user?.id ?? ""),
      )
    } catch {}
  }

  const handleRenameSession = async (sessionId: string) => {
    const name = window.prompt("Rename session:")
    if (name?.trim()) {
      try { await renameSession(user?.id ?? "", sessionId, name.trim()) } catch {}
      await refreshSessions()
    }
  }

  const handleArchiveSession = async (sessionId: string) => {
    try { await archiveSession(user?.id ?? "", sessionId) } catch {}
    await refreshSessions()
  }

  const handleUnarchiveSession = async (sessionId: string) => {
    try { await unarchiveSession(user?.id ?? "", sessionId) } catch {}
    await refreshSessions()
  }

  const newChat = async () => {
    const id = crypto.randomUUID()
    const tab: Tab = { id, title: "Chat", kind: "chat" }
    setTabs((prev) => [...prev, tab])
    setActiveTab(tab.id)
    await refreshSessions()
  }

  const switchToChat = (sessionId: string, name: string) => {
    const existing = tabs.find((t) => t.id === sessionId)
    if (existing) {
      setActiveTab(sessionId)
      return
    }
    setTabs((prev) => [...prev, { id: sessionId, title: name || "Chat", kind: "chat" }])
    setActiveTab(sessionId)
  }

  const openResearchTab = () => {
    setTabs((prev) => (
      prev.some((tab) => tab.id === "research")
        ? prev
        : [...prev, { id: "research", title: "Research", kind: "research" }]
    ))
    setActiveTab("research")
  }

  const deleteChat = async (sessionId: string) => {
    try { await deleteSession(user?.id ?? "", sessionId) } catch {}
    setTabs((prev) => prev.filter((t) => t.id !== sessionId))
    if (activeTab === sessionId) setActiveTab("chat")
    await refreshSessions()
  }

  // In host mode the file tree is the local folder, which only carries code.
  // The Paper/Research zones live cloud-only (created server-side, populated by
  // research saves), so we graft those remote top-level folders onto the local
  // tree — otherwise a host never sees their own saved references/papers.
  const hostTree = useMemo(() => {
    const local = tauriAgent.fileTree
    if (!local) return null
    const cloudZones = (remoteTree?.children ?? []).filter(
      (node) => node.type === "folder" && (node.zone === "research" || node.zone === "paper"),
    )
    if (cloudZones.length === 0) return local
    // Don't duplicate a zone the local folder already happens to contain.
    const localNames = new Set((local.children ?? []).map((child) => child.name))
    const grafted = cloudZones.filter((node) => !localNames.has(node.name))
    return { ...local, children: [...(local.children ?? []), ...grafted] }
  }, [tauriAgent.fileTree, remoteTree])

  const tree = workspaceMode === "guest" ? remoteTree || sampleTree : hostTree || sampleTree
  const active = tabs.find((tab) => tab.id === activeTab) || tabs[0]
  const activeFilePath = active?.kind === "file" ? active.id : undefined

  const openFile = async (file: FileTreeItem) => {
    const pathKey = normalizePathKey(file.path)
    // A grafted cloud zone (Paper/Research) carries a remote id even in host
    // mode; it lives only in the cloud, so it must be read/written remotely.
    const isCloudNode = Boolean(file.id) && (file.zone === "research" || file.zone === "paper")
    const remoteMode = workspaceMode === "guest" || isCloudNode
    const hostManifestEntry = workspaceMode === "host" && !isCloudNode
      ? loadSyncManifest(projectId, user?.id ?? "")[pathKey]
      : undefined
    const remoteContent = remoteMode && file.id
      ? await getProjectFileContent(projectId, file.id)
      : null
    const lang = fileLanguage(file)
    // Binary previews load bytes on their own.
    const content = lang === "pdf" || lang === "image"
      ? ""
      : remoteMode
        ? remoteContent?.content ?? null
        : await tauriAgent.openFile(file.path)
    const next: Tab = {
      id: file.path,
      title: file.name,
      kind: "file",
      language: lang,
      content: content ?? sampleContent,
      dirty: false,
      remoteFileId: remoteMode ? file.id : hostManifestEntry?.fileId,
      remoteUpdatedAt: remoteContent?.updated_at ?? hostManifestEntry?.remoteUpdatedAt ?? file.updated_at,
      saveStatus: "idle",
    }
    setTabs((prev) => prev.some((tab) => tab.id === next.id) ? prev.map((tab) => tab.id === next.id ? next : tab) : [...prev, next])
    setActiveTab(next.id)
  }

  const openEssay = (file: FileTreeItem) => {
    const essayFileId = (file as any).id || file.path
    const essayPath = file.path
    const params = new URLSearchParams()
    params.set("file", essayFileId)
    params.set("path", essayPath)
    router.push(`/projects/${projectId}/essay?${params.toString()}`)
  }

  const scheduleHostCollaborativeWrite = (path: string, content: string) => {
    if (workspaceMode !== "host") return
    const previous = localWriteTimers.current[path]
    if (previous) clearTimeout(previous)
    localWriteTimers.current[path] = setTimeout(() => {
      void tauriAgent.writeFile(path, content).catch(() => {})
      delete localWriteTimers.current[path]
    }, 700)
  }

  const updateActiveFileContent = (content: string, collaborative = false) => {
    setTabs((prev) => prev.map((tab) => (
      tab.id === activeTab && tab.kind === "file" && !tab.readOnly
        ? {
          ...tab,
          content,
          dirty: collaborative ? false : true,
          localExternalConflict: false,
          saveStatus: "idle",
        }
        : tab
    )))
    const fileTab = tabs.find((tab) => tab.id === activeTab && tab.kind === "file")
    if (collaborative && fileTab?.remoteFileId && workspaceMode === "host") {
      scheduleHostCollaborativeWrite(fileTab.id, content)
    }
  }

  // Live-react to external filesystem changes (e.g. a file edited in VSCode
  // while open here). The watcher fires one event per changed path; we match
  // it to any open tab and reconcile like a code editor would:
  //   - collaborative tab (remoteFileId) → Yjs owns the buffer, only flag it
  //   - plain tab, no unsaved edits      → adopt the new content live
  //   - plain tab with unsaved edits      → flag "changed on disk", offer Reload
  // Self-writes echo back with identical content, so they're a no-op.
  useEffect(() => {
    if (workspaceMode !== "host") return
    const unsubscribe = onFileChange((rawPath, content) => {
      const changedKey = normalizePathKey(rawPath)
      setTabs((prev) => prev.map((tab) => {
        if (tab.kind !== "file") return tab
        if (normalizePathKey(tab.id) !== changedKey) return tab
        if (content === tab.content) return tab
        if (tab.remoteFileId) {
          return { ...tab, localExternalConflict: true, saveStatus: "conflict" }
        }
        if (tab.dirty) {
          return { ...tab, externalChanged: true }
        }
        return { ...tab, content, externalChanged: false }
      }))
    })
    return unsubscribe
  }, [workspaceMode])

  // Compile a .tex tab to PDF. Saves any unsaved edits first (latexmk reads the
  // file from disk), then opens/refreshes the produced PDF. The file watcher's
  // file-binary-change event refreshes an already-open preview on its own.
  const compileLatexTab = async (tab: Tab) => {
    if (workspaceMode !== "host" || tab.kind !== "file" || latexCompiling) return
    if (tab.dirty) await saveActiveFile()
    setLatexCompiling(tab.id)
    setLatexError(null)
    try {
      const result = await compileLatex(tab.id)
      if (!result.success) {
        setLatexError({ path: tab.id, log: result.log || "Compilation failed." })
        return
      }
      await tauriAgent.refreshFileTree()
      if (result.pdf_path) {
        await openFile({
          name: result.pdf_path.split("/").pop() || "output.pdf",
          path: result.pdf_path,
          type: "file",
        })
      }
    } catch (error) {
      setLatexError({ path: tab.id, log: errorText(error, "LaTeX compile failed.") })
    } finally {
      setLatexCompiling(null)
    }
  }

  // Discard local edits and reload the on-disk version for the active tab.
  const reloadActiveFile = async () => {
    const fileTab = tabs.find((tab) => tab.id === activeTab && tab.kind === "file")
    if (!fileTab) return
    const content = await tauriAgent.openFile(fileTab.id)
    if (content == null) return
    setTabs((prev) => prev.map((tab) => (
      tab.id === fileTab.id
        ? { ...tab, content, dirty: false, externalChanged: false, saveStatus: "idle" }
        : tab
    )))
  }

  const saveActiveFile = async () => {
    const fileTab = tabs.find((tab) => tab.id === activeTab && tab.kind === "file")
    if (!fileTab || fileTab.readOnly || fileTab.saveStatus === "saving") return
    if (workspaceMode === "guest" && !canWriteFiles) return

    setTabs((prev) => prev.map((tab) => (
      tab.id === fileTab.id ? { ...tab, saveStatus: "saving" } : tab
    )))

    try {
      if (workspaceMode === "guest") {
        if (!fileTab.remoteFileId) throw new Error("remote file id missing")
        const saved = await updateProjectFileContent(
          projectId,
          fileTab.remoteFileId,
          fileTab.content ?? "",
          fileTab.remoteUpdatedAt,
        )
        setTabs((prev) => prev.map((tab) => (
          tab.id === fileTab.id
            ? { ...tab, dirty: false, remoteUpdatedAt: saved.updated_at, saveStatus: "saved" }
            : tab
        )))
        await refreshRemoteTree()
        return
      }

      await tauriAgent.writeFile(fileTab.id, fileTab.content ?? "")
      setTabs((prev) => prev.map((tab) => (
        tab.id === fileTab.id ? { ...tab, dirty: false, externalChanged: false, saveStatus: "saved" } : tab
      )))
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      const saveStatus = message.toLowerCase().includes("changed since") || message.includes("409")
        ? "conflict"
        : "error"
      setTabs((prev) => prev.map((tab) => (
        tab.id === fileTab.id ? { ...tab, saveStatus } : tab
      )))
    }
  }

  const removeSyncConflict = (conflict: SyncConflict) => {
    setSyncStats((prev) => prev
      ? { ...prev, conflicts: prev.conflicts.filter((item) => item.path !== conflict.path || item.fileId !== conflict.fileId) }
      : prev)
  }

  const keepRemoteConflict = async (conflict: SyncConflict) => {
    setConflictError(null)
    try {
      const remoteContent = await getProjectFileContent(projectId, conflict.fileId)
      const remoteHash = hashContent(remoteContent.content)
      const localContent = conflict.localDeleted ? null : await tauriAgent.openFile(conflict.path)
      const manifest = loadSyncManifest(projectId, user?.id ?? "")
      manifest[conflict.path] = {
        fileId: conflict.fileId,
        remoteUpdatedAt: remoteContent.updated_at,
        localHash: localContent == null ? "__deleted__" : hashContent(localContent),
        remoteHash,
        localDeleted: localContent == null,
      }
      saveSyncManifest(projectId, user?.id ?? "", manifest)
      removeSyncConflict(conflict)
      await refreshRemoteTree()
    } catch (error) {
      setConflictError(errorText(error, `Could not keep remote for ${conflict.path}.`))
    }
  }

  const overwriteRemoteConflict = async (conflict: SyncConflict) => {
    setConflictError(null)
    try {
      const localContent = await tauriAgent.openFile(conflict.path)
      const manifest = loadSyncManifest(projectId, user?.id ?? "")

      if (localContent == null) {
        await deleteProjectFile(projectId, conflict.fileId)
        delete manifest[conflict.path]
        saveSyncManifest(projectId, user?.id ?? "", manifest)
        removeSyncConflict(conflict)
        setSyncStats((prev) => prev ? { ...prev, deleted: prev.deleted + 1 } : prev)
        await refreshRemoteTree()
        return
      }

      const remoteContent = await getProjectFileContent(projectId, conflict.fileId)
      const saved = await updateProjectFileContent(projectId, conflict.fileId, localContent, remoteContent.updated_at)
      const localHash = hashContent(localContent)
      manifest[conflict.path] = {
        fileId: conflict.fileId,
        remoteUpdatedAt: saved.updated_at,
        localHash,
        remoteHash: localHash,
        localDeleted: false,
      }
      saveSyncManifest(projectId, user?.id ?? "", manifest)
      removeSyncConflict(conflict)
      setSyncStats((prev) => prev ? { ...prev, updated: prev.updated + 1 } : prev)
      await refreshRemoteTree()
    } catch (error) {
      setConflictError(errorText(error, `Could not overwrite remote for ${conflict.path}.`))
    }
  }

  const openDiffConflict = async (conflict: SyncConflict) => {
    setConflictError(null)
    try {
      const remoteContent = await getProjectFileContent(projectId, conflict.fileId)
      const localContent = conflict.localDeleted ? null : await tauriAgent.openFile(conflict.path)
      const name = conflict.path.split("/").pop() || conflict.path
      const nextTab: Tab = {
        id: `diff:${conflict.fileId}:${remoteContent.updated_at}`,
        title: `${name} diff`,
        kind: "diff",
        language: fileLanguage({ name, path: conflict.path, type: "file" }),
        diff: {
          left: localContent ?? "",
          right: remoteContent.content,
          leftTitle: conflict.localDeleted ? "Local deleted" : "Local",
          rightTitle: "Remote",
        },
        dirty: false,
        remoteFileId: conflict.fileId,
        remoteUpdatedAt: remoteContent.updated_at,
        readOnly: true,
        saveStatus: "idle",
      }

      setTabs((prev) => {
        return [...prev.filter((tab) => tab.id !== nextTab.id), nextTab]
      })
      setActiveTab(nextTab.id)
    } catch (error) {
      setConflictError(errorText(error, `Could not open diff for ${conflict.path}.`))
    }
  }

  const sidebarTitle = useMemo(() => {
    if (activeActivity === "explorer") {
      if (workspaceMode === "guest") return "Project Files"
      return tauriAgent.workDir?.split(/[/\\]/).pop() || "Explorer"
    }
    if (activeActivity === "arena") return "Arena"
    if (activeActivity === "knowledge") return "Knowledge Base"
    if (activeActivity === "research") return "Research"
    if (activeActivity === "chat") return "Chats"
    return "Settings"
  }, [activeActivity, tauriAgent.workDir, workspaceMode])

  return (
    <main className="flex h-screen overflow-hidden bg-[#0d0d0d] text-[#e8e8e8]">
      <nav className="flex w-12 flex-col items-center border-r border-[#373737] bg-[#121212] py-2">
        {activities.map((item) => (
          <button
            key={item.id}
            title={item.label}
            onClick={() => {
              setActiveActivity(item.id)
              if (item.id === "research") openResearchTab()
            }}
            className={cn(
              "relative flex h-11 w-12 items-center justify-center text-[#787878] hover:text-[#e8e8e8]",
              activeActivity === item.id && "text-[#e8e8e8]",
            )}
          >
            <item.icon className="h-5 w-5" />
            {activeActivity === item.id && <span className="absolute left-0 top-2 h-7 w-0.5 bg-[#d4a574]" />}
          </button>
        ))}
      </nav>

      <aside className="flex w-[260px] flex-col border-r border-[#373737] bg-[#1a1a1a]">
        <div className="flex h-10 items-center gap-2 border-b border-[#373737] px-3">
          <SidebarIcon className="h-4 w-4 text-[#787878]" />
          <span className="truncate text-xs font-medium uppercase tracking-wide text-[#b4b4b4]">{sidebarTitle}</span>
          {activeActivity === "explorer" && (
            <Button
              size="icon"
              variant="ghost"
              className="ml-auto h-7 w-7"
              onClick={workspaceMode === "guest" ? refreshRemoteTree : tauriAgent.refreshFileTree}
            >
              <RefreshCw className={cn("h-3.5 w-3.5", workspaceMode === "guest" && remoteStatus === "loading" && "animate-spin")} />
            </Button>
          )}
        </div>
        <ScrollArea className="min-h-0 flex-1">
          {activeActivity === "explorer" && (
            <>
              <div className="space-y-2 border-b border-[#373737] px-3 py-2">
                <div className="grid grid-cols-2 gap-1 rounded-md border border-[#373737] bg-[#121212] p-1">
                  {(["host", "guest"] as const).map((mode) => (
                    <button
                      key={mode}
                      onClick={() => setWorkspaceMode(mode)}
                      className={cn(
                        "rounded px-2 py-1 text-xs transition-colors",
                        workspaceMode === mode
                          ? "bg-[#d4a574] text-[#111111]"
                          : "text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8]",
                      )}
                    >
                      {mode === "host" ? "Host Local" : "Guest Remote"}
                    </button>
                  ))}
                </div>
                {workspaceMode === "host" ? (
                  <div className="space-y-1.5">
                    <button
                      onClick={openFolderAndSync}
                      disabled={!canSyncWorkspace}
                      className={cn(
                        "flex w-full items-center gap-2 rounded-md border border-[#373737] bg-[#232323] px-3 py-1.5 text-xs transition-colors",
                        canSyncWorkspace
                          ? "text-[#b4b4b4] hover:border-[#d4a574] hover:text-[#e8e8e8]"
                          : "cursor-not-allowed text-[#5f5f5f]",
                      )}
                    >
                      <FolderOpen className="h-3.5 w-3.5 text-[#d4a574]" />
                      Open Folder + Sync
                    </button>
                    <button
                      onClick={syncCurrentHostTree}
                      disabled={!tauriAgent.fileTree || syncing || !canSyncWorkspace}
                      className={cn(
                        "flex w-full items-center gap-2 rounded-md border border-[#373737] bg-[#232323] px-3 py-1.5 text-xs transition-colors",
                        tauriAgent.fileTree && !syncing && canSyncWorkspace
                          ? "text-[#b4b4b4] hover:border-[#d4a574] hover:text-[#e8e8e8]"
                          : "cursor-not-allowed text-[#5f5f5f]",
                      )}
                    >
                      <RefreshCw className={cn("h-3.5 w-3.5 text-[#d4a574]", syncing && "animate-spin")} />
                      {syncing ? "Syncing Remote" : "Sync to Remote"}
                    </button>
                  </div>
                ) : (
                  <button
                    onClick={refreshRemoteTree}
                    className="flex w-full items-center gap-2 rounded-md border border-[#373737] bg-[#232323] px-3 py-1.5 text-xs text-[#b4b4b4] transition-colors hover:border-[#d4a574] hover:text-[#e8e8e8]"
                  >
                    <RefreshCw className={cn("h-3.5 w-3.5 text-[#d4a574]", remoteStatus === "loading" && "animate-spin")} />
                    {remoteStatus === "loading" ? "Loading Remote" : "Refresh Remote"}
                  </button>
                )}
                {workspaceMode === "guest" && remoteStatus === "error" && (
                  <p className="text-[11px] leading-4 text-[#f44336]">Remote project files unavailable.</p>
                )}
                {workspaceMode === "host" && syncStats && (
                  <div className="space-y-2">
                    <p className="text-[11px] leading-4 text-[#787878]">
                      Synced: {syncStats.created} created, {syncStats.updated} updated, {syncStats.deleted} deleted, {syncStats.skipped} skipped, {syncStats.conflicts.length} conflicts, {syncStats.failed} failed.
                    </p>
                    {syncStats.conflicts.length > 0 && (
                      <div className="space-y-1 rounded-md border border-[#5f3f24] bg-[#1f1a14] p-2">
                        <p className="text-[11px] font-medium text-[#ebc396]">Conflicts need a decision</p>
                        {conflictError && (
                          <p role="alert" className="rounded border border-[#5f2424] bg-[#2d1a1a] px-1.5 py-1 text-[10px] leading-4 text-[#ffb4a8]">
                            {conflictError}
                          </p>
                        )}
                        {syncStats.conflicts.map((conflict) => (
                          <div key={`${conflict.fileId}:${conflict.path}`} className="space-y-1 border-t border-[#373737] pt-1 first:border-t-0 first:pt-0">
                            <p className="truncate font-mono text-[10px] text-[#b4b4b4]">{conflict.path}</p>
                            <div className="grid grid-cols-3 gap-1">
                              <button
                                onClick={() => keepRemoteConflict(conflict)}
                                className="rounded border border-[#373737] px-1.5 py-1 text-[10px] text-[#b4b4b4] hover:border-[#d4a574] hover:text-[#e8e8e8]"
                              >
                                Keep Remote
                              </button>
                              <button
                                onClick={() => overwriteRemoteConflict(conflict)}
                                className="rounded border border-[#5f3f24] px-1.5 py-1 text-[10px] text-[#ebc396] hover:bg-[#2d241a]"
                              >
                                Overwrite
                              </button>
                              <button
                                onClick={() => openDiffConflict(conflict)}
                                className="rounded border border-[#373737] px-1.5 py-1 text-[10px] text-[#b4b4b4] hover:border-[#d4a574] hover:text-[#e8e8e8]"
                              >
                                Diff
                              </button>
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
                {workspaceMode === "host" && !canSyncWorkspace && (
                  <p className="text-[11px] leading-4 text-[#787878]">workspace.sync and files.write permissions are required to publish local files.</p>
                )}
                {workspaceMode === "host" && hostFolderMissing && (
                  <p className="text-[11px] leading-4 text-[#ffb4a8]">Previous host folder is unavailable. Choose the folder again to resume auto sync.</p>
                )}
                {workspaceMode === "host" && canSyncWorkspace && autoSyncEnabled && (
                  <p className="text-[11px] leading-4 text-[#787878]">Auto sync is watching local file changes.</p>
                )}
              </div>
              <div className="py-1">
                <FileNode item={tree} depth={0} activePath={activeFilePath} onOpen={openFile} onOpenEssay={openEssay} />
              </div>
            </>
          )}
          {activeActivity === "arena" && (
            <button
              onClick={() => setActiveActivity("knowledge")}
              className="flex h-8 w-full items-center gap-2 px-3 text-left text-xs text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8] transition-colors"
            >
              <Library className="h-4 w-4 text-[#d4a574]" />
              Knowledge Base
            </button>
          )}
          {activeActivity === "knowledge" && (
            <button
              onClick={() => setActiveActivity("arena")}
              className="flex h-8 w-full items-center gap-2 px-3 text-left text-xs text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8] transition-colors"
            >
              <Network className="h-4 w-4 text-[#d4a574]" />
              Arena Cards
            </button>
          )}
          {activeActivity === "chat" && (
            <>
              {/* Search */}
              <div className="px-2 pb-1">
                <Input
                  placeholder="Search sessions..."
                  value={sessionQuery}
                  onChange={(e) => handleSessionSearch(e.target.value)}
                  className="h-7 text-xs border-[#373737] bg-[#1a1a1a] placeholder:text-[#787878]"
                />
              </div>
              <button
                onClick={() => { refreshSessions(); newChat() }}
                className="flex h-8 w-full items-center gap-2 px-3 text-left text-xs text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8] transition-colors"
              >
                <span className="text-[#d4a574]">+</span>
                New Chat
              </button>
              <div className="py-1">
                {/* Active sessions */}
                {sessions.filter((s) => s.status !== "archived" && !s.name.startsWith("[Research] ")).length === 0 ? (
                  <p className="px-3 py-4 text-center text-xs text-[#787878]">No conversations yet</p>
                ) : sessions.filter((s) => s.status !== "archived" && !s.name.startsWith("[Research] ")).map((s) => (
                  <div
                    key={s.id}
                    role="button"
                    tabIndex={0}
                    onClick={() => { refreshSessions(); switchToChat(s.id, s.name || "Chat") }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault()
                        refreshSessions()
                        switchToChat(s.id, s.name || "Chat")
                      }
                    }}
                    className="group flex h-8 w-full items-center gap-2 px-3 text-left text-xs text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8] transition-colors"
                  >
                    <MessageSquare className="h-3.5 w-3.5 text-[#d4a574]" />
                    <span className="flex-1 truncate">{s.name || new Date(s.created_at * 1000).toLocaleDateString()}</span>
                    <span className="hidden text-[#787878] group-hover:inline-flex">{s.message_count}</span>
                    <button
                      className="hidden h-4 w-4 items-center justify-center rounded text-[#787878] hover:bg-[#373737] hover:text-[#e8e8e8] group-hover:flex"
                      title="Rename"
                      onClick={(e) => { e.stopPropagation(); handleRenameSession(s.id) }}
                    >
                      <PencilLine className="h-3 w-3" />
                    </button>
                    <button
                      className="hidden h-4 w-4 items-center justify-center rounded text-[#787878] hover:bg-[#373737] hover:text-[#f59e0b] group-hover:flex"
                      title="Archive"
                      onClick={(e) => { e.stopPropagation(); handleArchiveSession(s.id) }}
                    >
                      <Archive className="h-3 w-3" />
                    </button>
                    <button
                      className="ml-1 hidden h-4 w-4 items-center justify-center rounded text-[#787878] hover:bg-[#373737] hover:text-[#f44336] group-hover:flex"
                      onClick={(e) => { e.stopPropagation(); deleteChat(s.id) }}
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </div>
                ))}
                {/* Archived section */}
                {sessions.some((s) => s.status === "archived" && !s.name.startsWith("[Research] ")) && (
                  <>
                    <button
                      onClick={() => setShowArchived((prev) => !prev)}
                      className="mt-2 flex h-7 w-full items-center gap-2 px-3 text-left text-xs text-[#787878] hover:text-[#b4b4b4] transition-colors"
                    >
                      {showArchived ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
                      Archived ({sessions.filter((s) => s.status === "archived" && !s.name.startsWith("[Research] ")).length})
                    </button>
                    {showArchived && sessions.filter((s) => s.status === "archived" && !s.name.startsWith("[Research] ")).map((s) => (
                      <div
                        key={s.id}
                        role="button"
                        tabIndex={0}
                        className="group flex h-8 w-full items-center gap-2 px-3 text-left text-xs text-[#787878] opacity-80 hover:bg-[#232323] hover:text-[#e8e8e8] hover:opacity-100 transition-colors"
                      >
                        <MessageSquare className="h-3.5 w-3.5 text-[#555]" />
                        <span className="flex-1 truncate">{s.name || new Date(s.created_at * 1000).toLocaleDateString()}</span>
                        <button
                          className="hidden h-4 w-4 items-center justify-center rounded text-[#787878] hover:bg-[#373737] hover:text-[#4caf50] group-hover:flex"
                          title="Restore"
                          onClick={(e) => { e.stopPropagation(); handleUnarchiveSession(s.id) }}
                        >
                          <RefreshCw className="h-3 w-3" />
                        </button>
                        <button
                          className="ml-1 hidden h-4 w-4 items-center justify-center rounded text-[#787878] hover:bg-[#373737] hover:text-[#f44336] group-hover:flex"
                          onClick={(e) => { e.stopPropagation(); deleteChat(s.id) }}
                        >
                          <X className="h-3 w-3" />
                        </button>
                      </div>
                    ))}
                  </>
                )}
              </div>
            </>
          )}
          {activeActivity === "settings" && <div className="p-3 text-xs text-[#787878]">Local configuration</div>}
        </ScrollArea>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        <div className="flex h-10 items-center overflow-x-auto border-b border-[#373737] bg-[#1a1a1a]">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                "flex h-full min-w-32 items-center gap-2 border-r border-[#373737] px-3 text-xs",
                activeTab === tab.id
                  ? "bg-[#0d0d0d] text-[#e8e8e8]"
                  : "text-[#b4b4b4] hover:bg-[#232323]",
              )}
            >
              {tab.kind === "chat" ? <MessageSquare className="h-4 w-4" /> : tab.kind === "research" ? <BookOpen className="h-4 w-4" /> : (tab as Tab).language === "pdf" ? <FileText className="h-4 w-4 text-[#f44336]" /> : (tab as Tab).language === "image" ? <FileImage className="h-4 w-4 text-[#64b5f6]" /> : <FileCode className="h-4 w-4" />}
              <span className="truncate">{tab.title}{tab.dirty ? " *" : ""}</span>
              <span
                role="button"
                className="ml-1 flex h-4 w-4 items-center justify-center rounded hover:bg-[#373737] cursor-pointer"
                onClick={(e) => {
                  e.stopPropagation()
                  setTabs((prev) => {
                    const next = prev.filter((t) => t.id !== tab.id)
                    if (activeTab === tab.id && next.length > 0) {
                      setActiveTab(next[Math.min(prev.indexOf(tab), next.length - 1)].id)
                    }
                    return next
                  })
                }}
              >
                <X className="h-3 w-3" />
              </span>
            </button>
          ))}
          {active?.kind === "file" && active.language === "latex" && workspaceMode === "host" && (
            <button
              onClick={() => compileLatexTab(active)}
              disabled={latexCompiling === active.id}
              title="Compile LaTeX to PDF (latexmk)"
              aria-label="Compile LaTeX to PDF"
              className={cn(
                "ml-auto mr-1.5 flex h-7 items-center gap-1.5 rounded-md border border-[#4caf50]/40 bg-[#4caf50]/15 px-2 text-xs text-[#9bd6b5] transition-colors hover:bg-[#4caf50]/25 hover:text-[#b6e6c6] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#4caf50] disabled:opacity-60",
              )}
            >
              {latexCompiling === active.id ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Play className="h-3.5 w-3.5 fill-current" />}
              {latexCompiling === active.id ? "Compiling" : "Compile"}
            </button>
          )}
          {active?.kind === "file" && (
            <button
              onClick={saveActiveFile}
              disabled={Boolean(active.remoteFileId) || active.readOnly || active.saveStatus === "saving" || (workspaceMode === "guest" && !canWriteFiles)}
              className={cn(
                "mr-2 flex h-7 items-center gap-1.5 rounded-md border px-2 text-xs transition-colors",
                active.language === "latex" && workspaceMode === "host" ? "" : "ml-auto",
                active.dirty
                  ? "border-[#d4a574] bg-[#2d241a] text-[#ebc396]"
                  : "border-[#373737] bg-[#232323] text-[#787878]",
                active.saveStatus === "saving" && "opacity-60",
              )}
            >
              <Save className="h-3.5 w-3.5" />
              {active.remoteFileId ? "Collaborative" : active.readOnly ? "Read only" : active.saveStatus === "saving" ? "Saving" : active.saveStatus === "conflict" ? "Conflict" : active.saveStatus === "error" ? "Save failed" : "Save"}
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1">
          {activeActivity === "settings" ? (
            <SettingsPanel
              projectId={projectId}
              project={project}
              capabilities={capabilities}
              currentUserId={currentUserId}
              screenShare={screenShare}
              onProjectRefresh={refreshProject}
            />
          ) : activeActivity === "arena" ? (
            <ArenaPanel projectId={projectId} capabilities={capabilities} />
          ) : activeActivity === "knowledge" ? (
            <KnowledgeBasePanel projectId={projectId} capabilities={capabilities} />
          ) : active?.kind === "chat" ? (
            <ChatPanel
              conversationId={active.id}
              projectId={projectId}
              workspaceMode={workspaceMode}
              capabilities={capabilities}
              onSessionPersisted={refreshSessions}
            />
          ) : active?.kind === "research" ? (
            <ResearchSearchPanel
              projectId={projectId}
              capabilities={capabilities}
              onKeepOpen={openResearchTab}
              workspaceMode={workspaceMode}
              hostFolder={workspaceMode === "host" ? tauriAgent.workDir : null}
            />
          ) : active?.kind === "diff" ? (
            <div className="flex h-full flex-col">
              <div className="flex h-8 items-center gap-3 border-b border-[#373737] bg-[#121212] px-3 text-xs text-[#b4b4b4]">
                <span>{active.diff?.leftTitle ?? "Local"}</span>
                <span className="text-[#787878]">vs</span>
                <span>{active.diff?.rightTitle ?? "Remote"}</span>
              </div>
              <div className="min-h-0 flex-1">
                <CodeEditor
                  language={active.language || "plaintext"}
                  value=""
                  diff={active.diff}
                />
              </div>
            </div>
          ) : active?.kind === "file" ? (
            <div className="flex h-full flex-col">
              {active.localExternalConflict && (
                <div className="border-b border-[#5f3f24] bg-[#2d241a] px-3 py-2 text-xs text-[#ebc396]">
                  Local file changed outside the collaborative editor. Yjs remains the source of truth; use Sync conflict controls before overwriting remote.
                </div>
              )}
              {active.saveStatus === "conflict" && (
                <div className="border-b border-[#5f3f24] bg-[#2d241a] px-3 py-2 text-xs text-[#ebc396]">
                  Remote file changed after you opened it. Refresh the remote tree and reopen the file before saving.
                </div>
              )}
              {active.externalChanged && (
                <div className="flex items-center justify-between gap-3 border-b border-[#5f3f24] bg-[#2d241a] px-3 py-2 text-xs text-[#ebc396]">
                  <span>This file changed on disk and you have unsaved edits. Reload discards your edits and loads the on-disk version.</span>
                  <button
                    type="button"
                    onClick={reloadActiveFile}
                    className="shrink-0 rounded border border-[#d4a574] px-2 py-0.5 text-[#ebc396] transition-colors hover:bg-[#3a2e1f] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#d4a574]"
                  >
                    Reload
                  </button>
                </div>
              )}
              {latexError && active.kind === "file" && latexError.path === active.id && (
                <div className="border-b border-[#5f2424] bg-[#2d1a1a] px-3 py-2 text-xs text-[#ffb4a8]">
                  <div className="mb-1 flex items-center justify-between gap-3">
                    <span className="font-medium">LaTeX compile failed</span>
                    <button
                      type="button"
                      onClick={() => setLatexError(null)}
                      aria-label="Dismiss compile error"
                      className="shrink-0 rounded px-1 text-[#ffb4a8] hover:bg-[#3d2424] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[#5f2424]"
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </div>
                  <pre className="max-h-40 overflow-auto whitespace-pre-wrap font-mono text-[10px] leading-4 text-[#e0a59c]">{latexError.log}</pre>
                </div>
              )}
              {active.saveStatus === "error" && (
                <div className="border-b border-[#5f2424] bg-[#2d1a1a] px-3 py-2 text-xs text-[#ffb4a8]">
                  Save failed. Check permissions and server status.
                </div>
              )}
              <div className="min-h-0 flex-1">
                {active.language === "pdf" ? (
                  <PdfViewer filePath={active.id} />
                ) : active.language === "image" ? (
                  <ImageViewer filePath={active.id} />
                ) : (
                  <CodeEditor
                    language={active.language || "plaintext"}
                    value={active.content || ""}
                    readOnly={active.readOnly || (workspaceMode === "guest" && !canWriteFiles)}
                    onChange={(value) => updateActiveFileContent(value, Boolean(active.remoteFileId))}
                    collaborative={active.remoteFileId ? {
                      fileId: active.remoteFileId,
                      user: { id: currentUserId ?? undefined, name: user?.display_name || "User" },
                      readOnly: active.readOnly || (workspaceMode === "guest" && !canWriteFiles),
                    } : undefined}
                  />
                )}
              </div>
            </div>
          ) : null}
        </div>
        <footer className="flex h-7 items-center justify-between border-t border-[#373737] bg-[#1a1a1a] px-3 text-xs text-[#787878]">
          <button
            onClick={signOut}
            className="flex items-center gap-1.5 text-[#787878] transition-colors hover:text-[#e8e8e8]"
          >
            <LogOut className="h-3.5 w-3.5" />
            Sign out
          </button>
          <span>{workspaceMode === "guest" ? "Guest: remote project" : "Host: local folder"} &middot; {project?.role ?? "unknown"}</span>
        </footer>
      </section>
    </main>
  )
}
