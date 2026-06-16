"use client"

import { useCallback, useEffect, useReducer, useRef, useState } from "react"
import { ArrowRight, CheckCircle2, ChevronDown, Loader2, Sparkles } from "lucide-react"
import {
  onResearchAgentDone,
  onResearchAgentError,
  onResearchAgentResults,
  onResearchAgentStream,
  onResearchAgentThinking,
  onResearchAgentTool,
  researchAgentRun,
  type AgentSource,
  type ResearchScraper,
} from "@/lib/tauri-api"
import { ResearchMarkdown } from "@/components/research/research-markdown"
import { SourceCard, ToolCard, type Phase, type ToolEntry } from "@/components/research/agent-cards"

interface AgentState {
  phase: Phase
  thinking: string
  tools: ToolEntry[]
  sources: AgentSource[]
  answer: string
  error: string
}

type Action =
  | { type: "start" }
  | { type: "thinking"; content: string }
  | { type: "tool"; entry: ToolEntry }
  | { type: "results"; sources: AgentSource[] }
  | { type: "stream"; content: string }
  | { type: "error"; message: string }
  | { type: "done" }

function reducer(state: AgentState, action: Action): AgentState {
  switch (action.type) {
    case "start":
      return { phase: "running", thinking: "", tools: [], sources: [], answer: "", error: "" }
    case "thinking":
      return { ...state, thinking: state.thinking + action.content }
    case "tool": {
      const tools = [...state.tools]
      const idx = tools.findIndex((t) => t.id === action.entry.id)
      if (idx >= 0) tools[idx] = action.entry
      else tools.push(action.entry)
      return { ...state, tools }
    }
    case "results": {
      // Merge by citation number, keeping order.
      const byCitation = new Map(state.sources.map((s) => [s.citation, s]))
      for (const s of action.sources) byCitation.set(s.citation, s)
      const sources = Array.from(byCitation.values()).sort((a, b) => a.citation - b.citation)
      return { ...state, sources }
    }
    case "stream":
      return { ...state, answer: state.answer + action.content }
    case "error":
      return { ...state, phase: "complete", error: action.message }
    case "done":
      return { ...state, phase: "complete" }
    default:
      return state
  }
}

const initialState: AgentState = {
  phase: "initial",
  thinking: "",
  tools: [],
  sources: [],
  answer: "",
  error: "",
}

export function AgentResearchView({
  scraper,
  onSaveSources,
}: {
  scraper: ResearchScraper
  onSaveSources?: (sources: AgentSource[]) => void
}) {
  const [query, setQuery] = useState("")
  const [state, dispatch] = useReducer(reducer, initialState)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [showThinking, setShowThinking] = useState(false)
  const requestIdRef = useRef<string | null>(null)
  const answerRef = useRef<HTMLDivElement>(null)

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

  useEffect(() => {
    if (state.phase === "running") {
      answerRef.current?.scrollTo({ top: answerRef.current.scrollHeight })
    }
  }, [state.answer, state.phase])

  const run = useCallback(async () => {
    const trimmed = query.trim()
    if (!trimmed || state.phase === "running") return
    const requestId = crypto.randomUUID()
    requestIdRef.current = requestId
    setSelected(new Set())
    dispatch({ type: "start" })
    try {
      await researchAgentRun(trimmed, requestId, scraper)
    } catch (err) {
      dispatch({ type: "error", message: err instanceof Error ? err.message : String(err) })
    }
  }, [query, scraper, state.phase])

  const toggleSource = (citation: number) =>
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(citation)) next.delete(citation)
      else next.add(citation)
      return next
    })

  const selectedSources = state.sources.filter((s) => selected.has(s.citation))

  return (
    <div className="flex h-full flex-col bg-[#0d0d0d]">
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

      <div ref={answerRef} className="flex-1 overflow-auto p-4">
        {state.phase === "initial" ? (
          <div className="flex h-full flex-col items-center justify-center text-center text-[#555]">
            <Sparkles className="mb-3 h-8 w-8 text-[#373737]" />
            <p className="text-sm">Agentic research — searches arXiv, Semantic Scholar, OpenAlex,</p>
            <p className="text-sm">Zenodo, Kaggle, and GitHub, then writes a cited answer.</p>
          </div>
        ) : (
          <div className="mx-auto max-w-3xl space-y-4">
            {/* Thinking (collapsible) */}
            {state.thinking && (
              <div className="rounded-lg border border-[#373737] bg-[#1a1a1a]">
                <button
                  onClick={() => setShowThinking((v) => !v)}
                  className="flex w-full items-center gap-2 px-3 py-2 text-xs text-[#787878]"
                >
                  <ChevronDown className={`h-3.5 w-3.5 transition-transform ${showThinking ? "" : "-rotate-90"}`} />
                  Reasoning
                </button>
                {showThinking && (
                  <div className="border-t border-[#373737] px-3 py-2 text-xs leading-relaxed text-[#787878] whitespace-pre-wrap">
                    {state.thinking}
                  </div>
                )}
              </div>
            )}

            {/* Tool timeline */}
            {state.tools.length > 0 && (
              <div className="space-y-1.5">
                {state.tools.map((t) => (
                  <ToolCard key={t.id} entry={t} />
                ))}
              </div>
            )}

            {/* Streaming answer */}
            {state.answer && (
              <div className="prose prose-invert max-w-none text-sm leading-relaxed text-[#e8e8e8]">
                <ResearchMarkdown content={state.answer} />
                {state.phase === "running" && (
                  <span className="ml-0.5 inline-block h-4 w-1.5 animate-pulse bg-[#d4a574] align-middle" />
                )}
              </div>
            )}

            {state.error && (
              <div className="rounded-md border border-[#5a3535] bg-[#2a1a1a] px-3 py-2 text-sm text-[#d49a9a]">
                {state.error}
              </div>
            )}

            {/* Sources */}
            {state.sources.length > 0 && (
              <div className="space-y-2 border-t border-[#373737] pt-3">
                <div className="flex items-center justify-between">
                  <h3 className="text-xs font-medium uppercase tracking-wide text-[#787878]">
                    Sources ({state.sources.length})
                  </h3>
                  {onSaveSources && selectedSources.length > 0 && (
                    <button
                      onClick={() => onSaveSources(selectedSources)}
                      className="flex items-center gap-1.5 rounded-md bg-[#d4a574] px-2.5 py-1 text-xs font-medium text-[#111111] hover:bg-[#ebc396]"
                    >
                      <CheckCircle2 className="h-3.5 w-3.5" />
                      Save {selectedSources.length}
                    </button>
                  )}
                </div>
                {state.sources.map((s) => (
                  <SourceCard
                    key={s.citation}
                    source={s}
                    selected={selected.has(s.citation)}
                    onToggle={() => toggleSource(s.citation)}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
