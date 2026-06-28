"use client"

import { useCallback, useEffect, useReducer, useRef, useState } from "react"
import {
  ArrowDown,
  ArrowRight,
  CheckCircle2,
  ChevronDown,
  Loader2,
  MessageSquare,
  Pencil,
  Plus,
  Sparkles,
  Trash2,
} from "lucide-react"
import { cn } from "@/lib/utils"
import {
  deleteSession,
  listSessions,
  loadSession,
  onResearchAgentDone,
  onResearchAgentError,
  onResearchAgentResults,
  onResearchAgentSourceUpdate,
  onResearchAgentStream,
  onResearchAgentThinking,
  onResearchAgentTool,
  renameSession,
  researchAgentRun,
  type AgentSource,
  type ResearchScraper,
  type SessionInfo,
} from "@/lib/tauri-api"
import { useAuth } from "@/hooks/use-auth"
import { ResearchMarkdown } from "@/components/research/research-markdown"
import { SourceCard, ToolCard, type Phase, type ToolEntry } from "@/components/research/agent-cards"

interface Turn {
  query: string
  thinking: string
  tools: ToolEntry[]
  sources: AgentSource[]
  answer: string
}

interface AgentState {
  phase: Phase
  // Completed turns (committed when "done" arrives). Read-only in render.
  turns: Turn[]
  // The streaming turn. Mutations from SSE/WS events land here until "done"
  // promotes it into `turns`.
  currentTurn: Turn
  error: string
}

type Action =
  | { type: "start"; query: string }
  | { type: "thinking"; content: string }
  | { type: "tool"; entry: ToolEntry }
  | { type: "results"; sources: AgentSource[] }
  | { type: "update_source"; citation: number; structured_data: Record<string, unknown> | null }
  | { type: "stream"; content: string }
  | { type: "commit" } // currentTurn → turns[], reset currentTurn
  | { type: "error"; message: string }
  | { type: "done" } // commit + phase=complete
  | { type: "loadHistory"; turns: Turn[] } // replace turns[] with loaded history
  | { type: "reset" }

const emptyTurn = (query = ""): Turn => ({
  query,
  thinking: "",
  tools: [],
  sources: [],
  answer: "",
})

function reducer(state: AgentState, action: Action): AgentState {
  switch (action.type) {
    case "start": {
      // If a previous turn is still in currentTurn (e.g. error mid-flight),
      // promote it to history before starting the new one.
      const turns =
        state.currentTurn.query || state.currentTurn.answer
          ? [...state.turns, state.currentTurn]
          : state.turns
      return {
        phase: "running",
        turns,
        currentTurn: emptyTurn(action.query),
        error: "",
      }
    }
    case "thinking":
      return {
        ...state,
        currentTurn: {
          ...state.currentTurn,
          thinking: state.currentTurn.thinking + action.content,
        },
      }
    case "tool": {
      const tools = [...state.currentTurn.tools]
      const idx = tools.findIndex((t) => t.id === action.entry.id)
      if (idx >= 0) tools[idx] = action.entry
      else tools.push(action.entry)
      return {
        ...state,
        currentTurn: { ...state.currentTurn, tools },
      }
    }
    case "results": {
      const byCitation = new Map(state.currentTurn.sources.map((s) => [s.citation, s]))
      for (const s of action.sources) byCitation.set(s.citation, s)
      const sources = Array.from(byCitation.values()).sort(
        (a, b) => a.citation - b.citation,
      )
      return {
        ...state,
        currentTurn: { ...state.currentTurn, sources },
      }
    }
    case "update_source": {
      const nextSources = state.currentTurn.sources.map((s) =>
        s.citation === action.citation
          ? { ...s, structured_data: action.structured_data }
          : s,
      )
      return { ...state, currentTurn: { ...state.currentTurn, sources: nextSources } }
    }
    case "stream":
      return {
        ...state,
        currentTurn: {
          ...state.currentTurn,
          answer: state.currentTurn.answer + action.content,
        },
      }
    case "commit":
      if (!state.currentTurn.query && !state.currentTurn.answer) return state
      return {
        ...state,
        turns: [...state.turns, state.currentTurn],
        currentTurn: emptyTurn(),
      }
    case "error":
      return { ...state, phase: "complete", error: action.message }
    case "done":
      // Promote currentTurn to history, then phase=complete.
      if (!state.currentTurn.query && !state.currentTurn.answer) {
        return { ...state, phase: "complete" }
      }
      return {
        phase: "complete",
        turns: [...state.turns, state.currentTurn],
        currentTurn: emptyTurn(),
        error: "",
      }
    case "loadHistory":
      // Replace turns[] with the loaded history. Reset currentTurn so the
      // streaming surface doesn't bleed in. If `turns` is empty (e.g. clicking
      // a brand-new session), phase goes back to "initial" so the empty
      // placeholder shows.
      return {
        phase: action.turns.length === 0 ? "initial" : "complete",
        turns: action.turns,
        currentTurn: emptyTurn(),
        error: "",
      }
    case "reset":
      return initialState
    default:
      return state
  }
}

const initialState: AgentState = {
  phase: "initial",
  turns: [],
  currentTurn: emptyTurn(),
  error: "",
}

export function AgentResearchView({
  scraper,
  onSaveSources,
}: {
  scraper: ResearchScraper
  onSaveSources?: (sources: AgentSource[]) => void
}) {
  const { user } = useAuth()
  const sessionUserId = user?.id ?? ""
  const [query, setQuery] = useState("")
  const [state, dispatch] = useReducer(reducer, initialState)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [showThinking, setShowThinking] = useState(false)
  const [showJumpBtn, setShowJumpBtn] = useState(false)
  // Bumped after each completed run to force the library to refetch.
  const [libraryRefreshKey, setLibraryRefreshKey] = useState(0)
  const requestIdRef = useRef<string | null>(null)
  // Namespaced conversation id; promoted to state so the library can switch
  // sessions and the load effect can run on changes.
  const [conversationId, setConversationId] = useState<string>(
    () => "research-" + crypto.randomUUID(),
  )
  const scrollRef = useRef<HTMLDivElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  // followTail lives in a ref so the scroll listener can read it without
  // re-running the streaming effect.
  const followTailRef = useRef<boolean>(true)

  // Load prior turns from disk whenever the conversation changes. Covers:
  //   - Initial mount (fresh id, no file → loadHistory([]) → "initial" empty)
  //   - Library selection (existing id → loadHistory(turns) → "complete")
  //   - "+ New" click (fresh id → same as initial)
  useEffect(() => {
    let cancelled = false
    loadSession(sessionUserId, conversationId)
      .then((session) => {
        if (cancelled) return
        dispatch({ type: "loadHistory", turns: sessionToTurns(session.messages) })
      })
      .catch((err) => {
        // Non-fatal: fall back to empty state so the user can still query.
        console.error("Failed to load research session:", err)
        dispatch({ type: "loadHistory", turns: [] })
      })
    return () => {
      cancelled = true
    }
  }, [conversationId])

  // Refresh the library after each completed run so newly saved turns show
  // up in the sidebar. Triggered by phase transitioning into "complete".
  useEffect(() => {
    if (state.phase === "complete") {
      setLibraryRefreshKey((k) => k + 1)
    }
  }, [state.phase])

  useEffect(() => {
    const cleanups = [
      onResearchAgentThinking((e) => {
        if (e.request_id !== requestIdRef.current) return
        dispatch({ type: "thinking", content: e.content })
      }),
      onResearchAgentTool((e) => {
        if (e.request_id !== requestIdRef.current) return
        dispatch({
          type: "tool",
          entry: { id: e.id, name: e.name, arguments: e.arguments, status: e.status, summary: e.summary },
        })
      }),
      onResearchAgentResults((e) => {
        if (e.request_id !== requestIdRef.current) return
        dispatch({ type: "results", sources: e.results })
        setSelected((prev) => {
          const next = new Set(prev)
          for (const s of e.results) next.add(s.citation)
          return next
        })
      }),
      onResearchAgentSourceUpdate((e) => {
        if (e.request_id !== requestIdRef.current) return
        dispatch({
          type: "update_source",
          citation: e.citation,
          structured_data: e.structured_data,
        })
      }),
      onResearchAgentStream((e) => {
        if (e.request_id !== requestIdRef.current) return
        if (e.content) dispatch({ type: "stream", content: e.content })
      }),
      onResearchAgentError((e) => {
        if (e.request_id !== requestIdRef.current) return
        dispatch({ type: "error", message: e.message })
      }),
      onResearchAgentDone((e) => {
        if (e.request_id !== requestIdRef.current) return
        dispatch({ type: "done" })
      }),
    ]
    return () => cleanups.forEach((fn) => fn())
  }, [])

  // ── Follow-tail scrolling ──
// Pattern: mirrors `arena-chat.tsx:362-417`. Auto-scroll ONLY when the user is
// already near the bottom. If they scroll up to read a prior chunk, leave them
// alone and surface a jump-to-latest button. New chunks arriving while they
// are scrolled up keep the button visible; once they manually scroll back near
// the bottom, the button disappears and follow-tail resumes.
const isNearBottom = useCallback(() => {
  const el = scrollRef.current
  if (!el) return true
  return el.scrollHeight - el.scrollTop - el.clientHeight < 150
}, [])

const scrollToBottom = useCallback(() => {
  bottomRef.current?.scrollIntoView()
  followTailRef.current = true
  setShowJumpBtn(false)
}, [])

useEffect(() => {
  if (state.phase !== "running") return
  const id = window.setTimeout(() => {
    if (followTailRef.current) {
      // Un-smoothed: smooth-scroll fights chunks arriving 30+/sec.
      bottomRef.current?.scrollIntoView()
    } else {
      setShowJumpBtn(true)
    }
  }, 50)
  return () => window.clearTimeout(id)
}, [state.currentTurn.answer, state.phase])

useEffect(() => {
  const el = scrollRef.current
  if (!el) return
  const onScroll = () => {
    if (isNearBottom()) {
      followTailRef.current = true
      setShowJumpBtn(false)
    }
  }
  el.addEventListener("scroll", onScroll, { passive: true })
  return () => el.removeEventListener("scroll", onScroll)
}, [isNearBottom])

useEffect(() => {
  if (state.phase === "running") {
    followTailRef.current = true
    setShowJumpBtn(false)
  } else if (state.phase === "complete" && isNearBottom()) {
    setShowJumpBtn(false)
  }
}, [state.phase, isNearBottom])

  const run = useCallback(async () => {
    const trimmed = query.trim()
    if (!trimmed || state.phase === "running") return
    const requestId = crypto.randomUUID()
    requestIdRef.current = requestId
    setSelected(new Set())
    dispatch({ type: "start", query: trimmed })
    try {
      await researchAgentRun(
        trimmed,
        requestId,
        conversationId,
        scraper,
        sessionUserId || null,
      )
    } catch (err) {
      dispatch({ type: "error", message: err instanceof Error ? err.message : String(err) })
    }
    // The backend's `research_agent:done` event triggers `dispatch({type:"done"})`,
    // which commits currentTurn → turns[].
  }, [query, scraper, state.phase, conversationId])

  const startNewResearch = useCallback(() => {
    if (state.phase === "running") return
    setConversationId("research-" + crypto.randomUUID())
  }, [state.phase])

  const openResearchSession = useCallback(
    (id: string) => {
      if (state.phase === "running") return
      setConversationId(id)
    },
    [state.phase],
  )

  const toggleSource = (citation: number) =>
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(citation)) next.delete(citation)
      else next.add(citation)
      return next
    })

  const selectedSources = state.currentTurn.sources.filter((s) =>
    selected.has(s.citation),
  )

  return (
    <div className="flex h-full bg-[#0d0d0d]">
      <ResearchLibrary
        currentId={conversationId}
        onSelect={openResearchSession}
        onNew={startNewResearch}
        refreshKey={libraryRefreshKey}
        disabled={state.phase === "running"}
      />

      <div className="flex min-w-0 flex-1 flex-col">
        {/* Query bar */}
        <div className="border-b border-[#373737] p-3">
          <div className="flex items-center gap-2 rounded-md border border-[#373737] bg-[#232323] px-3 focus-within:border-[#d4a574]">
            <Sparkles className="h-4 w-4 shrink-0 text-[#d4a574]" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && run()}
              placeholder="Ask a research question — the agent will search and synthesize an answer"
              className="flex-1 bg-transparent py-2.5 text-sm text-[#e8e8e8] outline-none placeholder:text-[#555]"
            />
            <button
              onClick={run}
              disabled={state.phase === "running" || !query.trim()}
              className="rounded p-1 text-[#d4a574] hover:bg-[#2a2a2a] disabled:opacity-40"
            >
              {state.phase === "running" ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <ArrowRight className="h-4 w-4" />
              )}
            </button>
          </div>
        </div>

        <div ref={scrollRef} className="relative flex-1 overflow-auto p-4">
          {state.phase === "initial" && state.turns.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-center text-[#555]">
              <Sparkles className="mb-3 h-8 w-8 text-[#373737]" />
              <p className="text-sm">Agentic research — searches arXiv, Semantic Scholar, OpenAlex,</p>
              <p className="text-sm">Zenodo, Kaggle, and GitHub, then writes a cited answer.</p>
            </div>
          ) : (
            <div className="mx-auto max-w-3xl space-y-6">
              {/* Completed turns — inline, read-only above the live turn */}
              {state.turns.map((turn, i) => (
                <TurnCard key={`turn-${i}`} turn={turn} />
              ))}

              {/* Current streaming turn */}
              {(state.currentTurn.query || state.phase === "running") && (
                <CurrentTurnView
                  turn={state.currentTurn}
                  phase={state.phase}
                  showThinking={showThinking}
                  onToggleThinking={() => setShowThinking((v) => !v)}
                  selected={selected}
                  onToggleSource={toggleSource}
                  onSaveSources={onSaveSources ? () => onSaveSources(selectedSources) : undefined}
                  selectedSourcesCount={selectedSources.length}
                />
              )}

              {state.error && (
                <div className="rounded-md border border-[#5a3535] bg-[#2a1a1a] px-3 py-2 text-sm text-[#d49a9a]">
                  {state.error}
                </div>
              )}
            </div>
          )}
          {/* Scroll target for follow-tail and the jump button. */}
          <div ref={bottomRef} />
          {showJumpBtn && state.phase !== "initial" && (
            <button
              onClick={scrollToBottom}
              className="sticky bottom-3 float-right mr-1 rounded-full bg-[#d4a574] p-2 text-[#111111] shadow-lg hover:bg-[#ebc396]"
              aria-label="Jump to latest"
            >
              <ArrowDown className="h-4 w-4" />
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

// ── Helpers ─────────────────────────────────────────────────────────

/**
 * Convert stored session messages back into Turn[]. Pairs each user message
 * with the immediately following assistant message. Drops pairs where the
 * assistant begins with `⚠` (error marker). Strips the `[Research] `
 * prefix from user content so the display matches what the user originally
 * typed.
 */
function sessionToTurns(
  messages: { role: string; content?: string | null }[],
): Turn[] {
  const turns: Turn[] = []
  for (let i = 0; i < messages.length; i++) {
    const m = messages[i]
    if (m.role !== "user") continue
    const userContent = m.content ?? ""
    const query = userContent.startsWith("[Research] ")
      ? userContent.slice("[Research] ".length)
      : userContent
    const next = messages[i + 1]
    if (next && next.role === "assistant") {
      const ansContent = next.content ?? ""
      if (ansContent.startsWith("⚠")) {
        // Error pair: skip both
        i += 1
        continue
      }
      turns.push({ ...emptyTurn(query), answer: ansContent })
      i += 1
    } else {
      // User without assistant (interrupted mid-stream): keep the query.
      turns.push({ ...emptyTurn(query), answer: "" })
    }
  }
  return turns
}

// ── Library (left-side panel) ───────────────────────────────────────

function ResearchLibrary({
  currentId,
  onSelect,
  onNew,
  refreshKey,
  disabled,
}: {
  currentId: string
  onSelect: (id: string) => void
  onNew: () => void
  refreshKey: number
  disabled?: boolean
}) {
  const { user } = useAuth()
  const sessionUserId = user?.id ?? ""
  const [sessions, setSessions] = useState<SessionInfo[]>([])

  const refresh = useCallback(async () => {
    try {
      const all = await listSessions(sessionUserId)
      setSessions(
        all.filter((s) => (s.name || "").startsWith("[Research] ")),
      )
    } catch (err) {
      console.error("ResearchLibrary: listSessions failed:", err)
    }
  }, [sessionUserId])

  useEffect(() => {
    refresh()
  }, [refresh, refreshKey])

  // Strip the "[Research] " prefix the Rust side uses to mark research sessions.
  const stripPrefix = (name: string) =>
    (name || "Untitled").replace(/^\[Research\]\s+/, "")

  // Delete with native confirm. After deletion, force a re-fetch so the
  // current session in the parent doesn't point at a missing record.
  const handleDelete = async (s: SessionInfo, e: React.MouseEvent) => {
    e.stopPropagation()
    e.preventDefault()
    if (disabled) return
    const displayName = stripPrefix(s.name)
    const ok = window.confirm(
      `Delete research session "${displayName}"? This removes all ${s.message_count} messages and cannot be undone.`,
    )
    if (!ok) return
    try {
      await deleteSession(sessionUserId, s.id)
      await refresh()
    } catch (err) {
      console.error("ResearchLibrary: deleteSession failed:", err)
      window.alert(`Failed to delete session: ${err}`)
    }
  }

  // Rename via prompt(). Re-add the "[Research] " prefix on save so the
  // session stays classified as a research session in listSessions().
  const handleRename = async (s: SessionInfo, e: React.MouseEvent) => {
    e.stopPropagation()
    e.preventDefault()
    if (disabled) return
    const currentDisplay = stripPrefix(s.name)
    const next = window.prompt("Rename research session:", currentDisplay)
    if (next === null) return // cancelled
    const trimmed = next.trim()
    if (!trimmed || trimmed === currentDisplay) return
    try {
      await renameSession(sessionUserId, s.id, `[Research] ${trimmed}`)
      await refresh()
    } catch (err) {
      console.error("ResearchLibrary: renameSession failed:", err)
      window.alert(`Failed to rename session: ${err}`)
    }
  }

  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-[#373737] bg-[#0a0a0a]">
      <div className="flex items-center justify-between border-b border-[#373737] p-3">
        <h2 className="text-xs font-medium uppercase tracking-wide text-[#787878]">
          Library
        </h2>
        <button
          onClick={onNew}
          disabled={disabled}
          className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-[#d4a574] hover:bg-[#1a1a1a] disabled:opacity-40 disabled:hover:bg-transparent"
          title="Start a new research session"
        >
          <Plus className="h-3 w-3" />
          New
        </button>
      </div>
      <div className="flex-1 overflow-y-auto py-1">
        {sessions.length === 0 ? (
          <p className="px-3 py-4 text-center text-xs text-[#555]">
            No research yet
          </p>
        ) : (
          sessions.map((s) => {
            const isCurrent = s.id === currentId
            return (
              <div
                key={s.id}
                className={cn(
                  "group flex w-full flex-col gap-0.5 border-b border-[#1a1a1a] px-3 py-2 text-left transition-colors hover:bg-[#1a1a1a]",
                  isCurrent && "bg-[#1a1a1a]",
                  disabled && "opacity-40",
                )}
              >
                <button
                  onClick={() => !disabled && onSelect(s.id)}
                  disabled={disabled}
                  className="flex flex-1 items-center gap-1.5 text-left disabled:cursor-not-allowed"
                >
                  <MessageSquare className="h-3 w-3 shrink-0 text-[#d4a574]" />
                  <span className="flex-1 truncate text-xs text-[#e8e8e8]">
                    {stripPrefix(s.name)}
                  </span>
                </button>
                <div className="flex items-center justify-between gap-1 pl-[22px]">
                  <span className="text-[10px] text-[#555]">
                    {s.message_count} msgs ·{" "}
                    {new Date(s.created_at * 1000).toLocaleDateString()}
                  </span>
                  <div
                    className={cn(
                      "flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100",
                      isCurrent && "opacity-100",
                    )}
                  >
                    <button
                      onClick={(e) => handleRename(s, e)}
                      disabled={disabled}
                      title="Rename session"
                      aria-label={`Rename ${stripPrefix(s.name)}`}
                      className="rounded p-0.5 text-[#787878] hover:bg-[#262626] hover:text-[#d4a574] disabled:cursor-not-allowed disabled:opacity-40"
                    >
                      <Pencil className="h-3 w-3" />
                    </button>
                    <button
                      onClick={(e) => handleDelete(s, e)}
                      disabled={disabled}
                      title="Delete session"
                      aria-label={`Delete ${stripPrefix(s.name)}`}
                      className="rounded p-0.5 text-[#787878] hover:bg-[#262626] hover:text-[#d96b6b] disabled:cursor-not-allowed disabled:opacity-40"
                    >
                      <Trash2 className="h-3 w-3" />
                    </button>
                  </div>
                </div>
              </div>
            )
          })
        )}
      </div>
    </aside>
  )
}

function TurnCard({ turn }: { turn: Turn }) {
  return (
    <div className="space-y-3 border-b border-[#373737] pb-4">
      {/* User query */}
      <div className="rounded-md border border-[#373737] bg-[#1a1a1a] px-3 py-2 text-sm text-[#e8e8e8]">
        {turn.query}
      </div>

      {/* Reasoning (collapsed by default for past turns) */}
      {turn.thinking && <TurnThinking thinking={turn.thinking} />}

      {/* Tool timeline */}
      {turn.tools.length > 0 && (
        <div className="space-y-1.5">
          {turn.tools.map((t) => (
            <ToolCard key={t.id} entry={t} />
          ))}
        </div>
      )}

      {/* Final answer */}
      {turn.answer && (
        <div className="prose prose-invert max-w-none text-sm leading-relaxed text-[#e8e8e8]">
          <ResearchMarkdown content={turn.answer} />
        </div>
      )}

      {/* Sources — collapsed, citations visible for follow-up referencing */}
      {turn.sources.length > 0 && (
        <details className="rounded-md border border-[#373737] bg-[#0d0d0d]">
          <summary className="cursor-pointer px-3 py-2 text-xs font-medium uppercase tracking-wide text-[#787878] hover:text-[#b4b4b4]">
            Sources ({turn.sources.length})
          </summary>
          <div className="space-y-2 border-t border-[#373737] p-2">
            {turn.sources.map((s) => (
              <SourceCard
                key={s.citation}
                source={s}
                selected={false}
                onToggle={() => {
                  /* read-only past turn */
                }}
              />
            ))}
          </div>
        </details>
      )}
    </div>
  )
}

function TurnThinking({ thinking }: { thinking: string }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="rounded-lg border border-[#373737] bg-[#1a1a1a]">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-2 text-xs text-[#787878]"
      >
        <ChevronDown className={`h-3.5 w-3.5 transition-transform ${open ? "" : "-rotate-90"}`} />
        Reasoning
      </button>
      {open && (
        <div className="border-t border-[#373737] px-3 py-2 text-xs leading-relaxed text-[#787878] whitespace-pre-wrap">
          {thinking}
        </div>
      )}
    </div>
  )
}

// ── Current-turn renderer (live, with blinking cursor) ──────────────

function CurrentTurnView({
  turn,
  phase,
  showThinking,
  onToggleThinking,
  selected,
  onToggleSource,
  onSaveSources,
  selectedSourcesCount,
}: {
  turn: Turn
  phase: Phase
  showThinking: boolean
  onToggleThinking: () => void
  selected: Set<number>
  onToggleSource: (citation: number) => void
  onSaveSources?: () => void
  selectedSourcesCount: number
}) {
  return (
    <div className="space-y-3">
      {/* User query */}
      {turn.query && (
        <div className="rounded-md border border-[#373737] bg-[#1a1a1a] px-3 py-2 text-sm text-[#e8e8e8]">
          {turn.query}
        </div>
      )}

      {/* Reasoning (visibility controlled by parent so streaming toggles work) */}
      {turn.thinking && (
        <div className="rounded-lg border border-[#373737] bg-[#1a1a1a]">
          <button
            onClick={onToggleThinking}
            className="flex w-full items-center gap-2 px-3 py-2 text-xs text-[#787878]"
          >
            <ChevronDown className={`h-3.5 w-3.5 transition-transform ${showThinking ? "" : "-rotate-90"}`} />
            Reasoning
          </button>
          {showThinking && (
            <div className="border-t border-[#373737] px-3 py-2 text-xs leading-relaxed text-[#787878] whitespace-pre-wrap">
              {turn.thinking}
            </div>
          )}
        </div>
      )}

      {/* Tool timeline */}
      {turn.tools.length > 0 && (
        <div className="space-y-1.5">
          {turn.tools.map((t) => (
            <ToolCard key={t.id} entry={t} />
          ))}
        </div>
      )}

      {/* Streaming answer */}
      {turn.answer && (
        <div className="prose prose-invert max-w-none text-sm leading-relaxed text-[#e8e8e8]">
          <ResearchMarkdown content={turn.answer} />
          {phase === "running" && (
            <span className="ml-0.5 inline-block h-4 w-1.5 animate-pulse bg-[#d4a574] align-middle" />
          )}
        </div>
      )}

      {/* Sources (live, with save button) */}
      {turn.sources.length > 0 && (
        <div className="space-y-2 border-t border-[#373737] pt-3">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-medium uppercase tracking-wide text-[#787878]">
              Sources ({turn.sources.length})
            </h3>
            {onSaveSources && selectedSourcesCount > 0 && (
              <button
                onClick={onSaveSources}
                className="flex items-center gap-1.5 rounded-md bg-[#d4a574] px-2.5 py-1 text-xs font-medium text-[#111111] hover:bg-[#ebc396]"
              >
                <CheckCircle2 className="h-3.5 w-3.5" />
                Save {selectedSourcesCount}
              </button>
            )}
          </div>
          {turn.sources.map((s) => (
            <SourceCard
              key={s.citation}
              source={s}
              selected={selected.has(s.citation)}
              onToggle={() => onToggleSource(s.citation)}
            />
          ))}
        </div>
      )}
    </div>
  )
}
