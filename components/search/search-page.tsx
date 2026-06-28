"use client"

import { useState, useRef, useEffect, useCallback } from "react"
import { ArrowRight, ExternalLink, Loader2, Search, Sparkles, Clock, Copy, CheckCircle2 } from "lucide-react"
import { useRouter } from "next/navigation"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter"
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism"
import {
  aiSearch,
  onSearchStream,
  onSearchResults,
  onSearchQuestions,
  onSearchError,
  type SearchResultItem,
  type SearchStreamEvent,
  type SearchResultsEvent,
  type SearchQuestionsEvent,
} from "@/lib/tauri-api"
import { useAuth } from "@/hooks/use-auth"

// ─── storage ───
//
// History is keyed by user_id so two accounts sharing the same
// browser don't see each other's search queries (which can be
// sensitive — "company X financials", "competitor Y patents",
// etc.). When the auth profile is missing we fall back to a
// session-local "anon" bucket rather than the old shared key.

const MAX_HISTORY = 20

interface HistoryEntry {
  query: string
  timestamp: number
}

function historyKey(userId: string) {
  return userId ? `search_history:${userId}` : "search_history:anon"
}

function loadHistory(userId: string): HistoryEntry[] {
  if (typeof window === "undefined") return []
  try {
    const raw = localStorage.getItem(historyKey(userId))
    return raw ? (JSON.parse(raw) as HistoryEntry[]) : []
  } catch {
    return []
  }
}

function saveHistory(userId: string, query: string) {
  const history = loadHistory(userId).filter((e) => e.query !== query)
  history.unshift({ query, timestamp: Date.now() })
  localStorage.setItem(historyKey(userId), JSON.stringify(history.slice(0, MAX_HISTORY)))
}

// ─── citation parser ───

function CitationLink({ num }: { num: number }) {
  return (
    <sup>
      <a
        href={`#ref-${num}`}
        className="inline-flex items-center justify-center rounded-full bg-[#1a1a1a] border border-[#373737] px-1 text-[10px] font-medium text-[#d4a574] hover:bg-[#2a2a2a] no-underline"
      >
        {num}
      </a>
    </sup>
  )
}

// ─── markdown renderer ───

function MarkdownContent({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        code({ className, children, ...props }) {
          const match = /language-(\w+)/.exec(className || "")
          const code = String(children).replace(/\n$/, "")
          const isInline = !match && !code.includes("\n")
          if (isInline) {
            return (
              <code className="rounded bg-[#232323] px-1.5 py-0.5 text-[13px] text-[#d4a574]" {...props}>
                {children}
              </code>
            )
          }
          return (
            <div className="my-2 overflow-hidden rounded-lg border border-[#373737]">
              <div className="flex items-center justify-between bg-[#1e1e1e] px-3 py-1.5 text-xs text-[#787878]">
                <span>{match?.[1] || "text"}</span>
                <button
                  className="hover:text-[#e8e8e8]"
                  onClick={() => navigator.clipboard.writeText(code)}
                >
                  Copy
                </button>
              </div>
              <SyntaxHighlighter
                language={match?.[1] || "text"}
                style={vscDarkPlus}
                customStyle={{ margin: 0, borderRadius: 0, fontSize: "13px" }}
              >
                {code}
              </SyntaxHighlighter>
            </div>
          )
        },
        sup({ children }) {
          // Convert [1], [2] style citations to clickable links
          const text = String(children)
          const match = text.match(/^\[(\d+)\]$/)
          if (match) {
            return <CitationLink num={parseInt(match[1]!, 10)} />
          }
          return <sup>{children}</sup>
        },
        a({ href, children }) {
          return (
            <a href={href} target="_blank" rel="noopener noreferrer" className="text-[#d4a574] underline">
              {children}
            </a>
          )
        },
        h1({ children }) {
          return <h1 className="mt-4 mb-2 text-xl font-semibold">{children}</h1>
        },
        h2({ children }) {
          return <h2 className="mt-3 mb-1.5 text-lg font-semibold">{children}</h2>
        },
        h3({ children }) {
          return <h3 className="mt-2 mb-1 text-base font-semibold">{children}</h3>
        },
        p({ children }) {
          return <p className="mb-2 leading-relaxed">{children}</p>
        },
        ul({ children }) {
          return <ul className="mb-2 list-disc pl-5 space-y-1">{children}</ul>
        },
        ol({ children }) {
          return <ol className="mb-2 list-decimal pl-5 space-y-1">{children}</ol>
        },
        blockquote({ children }) {
          return (
            <blockquote className="border-l-2 border-[#d4a574] bg-[#1a1a1a] px-3 py-1 my-2 italic text-[#b4b4b4]">
              {children}
            </blockquote>
          )
        },
      }}
    >
      {content}
    </ReactMarkdown>
  )
}

// ─── result card ───

function SearchResultCard({ result, index }: { result: SearchResultItem; index: number }) {
  return (
    <div id={`ref-${index}`} className="rounded-lg border border-[#373737] bg-[#0d0d0d] px-4 py-3">
      <div className="flex items-start gap-2">
        <span className="mt-0.5 shrink-0 text-[11px] font-medium text-[#555]">[{index}]</span>
        <div className="min-w-0">
          <a
            href={result.url}
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm font-medium text-[#d4a574] underline underline-offset-2"
          >
            {result.title}
          </a>
          <p className="mt-0.5 truncate text-[10px] font-mono text-[#555]">{result.url}</p>
          <p className="mt-1.5 text-xs leading-relaxed text-[#b4b4b4] line-clamp-3">{result.content}</p>
        </div>
        <div className="ml-auto shrink-0 flex items-center gap-1">
          <a
            href={result.url}
            target="_blank"
            rel="noopener noreferrer"
            className="rounded p-1 text-[#555] hover:bg-[#1a1a1a] hover:text-[#e8e8e8]"
          >
            <ExternalLink className="h-3.5 w-3.5" />
          </a>
        </div>
      </div>
    </div>
  )
}

// ─── main page ───

export default function SearchPage() {
  const { user } = useAuth()
  const sessionUserId = user?.id ?? ""
  const [query, setQuery] = useState("")
  const [phase, setPhase] = useState<"initial" | "searching" | "streaming" | "complete">("initial")
  const [answer, setAnswer] = useState("")
  const [results, setResults] = useState<SearchResultItem[]>([])
  const [questions, setQuestions] = useState<string[]>([])
  const [error, setError] = useState("")
  const [history, setHistory] = useState<HistoryEntry[]>([])
  const scrollRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const currentSearchIdRef = useRef<string | null>(null)

  useEffect(() => {
    setHistory(loadHistory(sessionUserId))
  }, [])

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const scrollToBottom = useCallback(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" })
  }, [])

  useEffect(() => {
    scrollToBottom()
  }, [answer, scrollToBottom])

  useEffect(() => {
    const cleanup = [
      onSearchResults((event: SearchResultsEvent) => {
        if (event.request_id !== currentSearchIdRef.current) return
        setResults(event.results)
        setPhase("streaming")
      }),
      onSearchStream((event: SearchStreamEvent) => {
        if (event.request_id !== currentSearchIdRef.current) return
        setAnswer((prev) => prev + event.content)
        if (event.done) {
          setPhase("complete")
        }
      }),
      onSearchQuestions((event: SearchQuestionsEvent) => {
        if (event.request_id !== currentSearchIdRef.current) return
        setQuestions(event.questions)
      }),
      onSearchError((event) => {
        if (event.request_id !== currentSearchIdRef.current) return
        setError(event.message)
        setPhase("complete")
      }),
    ]

    return () => cleanup.forEach((fn) => fn())
  }, [])

  const handleSearch = async (searchQuery?: string) => {
    const q = (searchQuery || query).trim()
    if (!q) return
    const requestId = crypto.randomUUID()
    currentSearchIdRef.current = requestId

    setQuery(q)
    setAnswer("")
    setResults([])
    setQuestions([])
    setError("")
    setPhase("searching")

    saveHistory(sessionUserId, q)
    setHistory(loadHistory(sessionUserId))

    try {
      await aiSearch(q, requestId)
    } catch (e) {
      if (currentSearchIdRef.current !== requestId) return
      setError(String(e))
      setPhase("complete")
    }
  }

  const handleHistoryClick = (entry: HistoryEntry) => {
    setQuery(entry.query)
    void handleSearch(entry.query)
  }

  const handleQuestionClick = (question: string) => {
    setQuery(question)
    void handleSearch(question)
  }

  return (
    <div className="flex h-screen flex-col bg-[#0d0d0d] text-[#e8e8e8]">
      {/* Header */}
      <header className="flex h-12 items-center gap-3 border-b border-[#373737] bg-[#121212]/95 px-4 shrink-0">
        <Sparkles className="h-5 w-5 text-[#d4a574]" />
        <span className="text-sm font-medium">AI Search</span>
        <span className="text-[10px] uppercase tracking-[0.18em] text-[#787878]">Tavily + DeepSeek V4</span>
      </header>

      {/* Main content */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        {phase === "initial" ? (
          <InitialState
            history={history}
            onHistoryClick={handleHistoryClick}
          />
        ) : (
          <ResultsState
            phase={phase}
            query={query}
            answer={answer}
            results={results}
            questions={questions}
            error={error}
            onQuestionClick={handleQuestionClick}
            history={history}
            onHistoryClick={handleHistoryClick}
          />
        )}
      </div>

      {/* Search box — bottom when initial, top-anchored after search */}
      <div
        className={
          phase === "initial"
            ? "flex-1 flex flex-col items-center justify-center px-4 -mt-12"
            : "shrink-0 border-t border-[#373737] bg-[#121212]/95 px-4 py-3"
        }
      >
        <SearchBox
          query={query}
          onQueryChange={setQuery}
          onSearch={() => handleSearch()}
          loading={phase === "searching" || phase === "streaming"}
          compact={phase !== "initial"}
        />
        {phase === "initial" && (
          <p className="mt-6 text-xs text-[#555]">
            Tavily Search + DeepSeek V4 · Streaming answers with citations
          </p>
        )}
      </div>
    </div>
  )
}

// ─── sub-components ───

function SearchBox({
  query,
  onQueryChange,
  onSearch,
  loading,
  compact,
}: {
  query: string
  onQueryChange: (v: string) => void
  onSearch: () => void
  loading: boolean
  compact: boolean
}) {
  return (
    <div className={compact ? "w-full" : "w-full max-w-2xl"}>
      <div className="flex items-center gap-2 rounded-xl border border-[#373737] bg-[#1a1a1a] px-4 py-3 focus-within:border-[#d4a574] transition-colors">
        {loading ? (
          <Loader2 className="h-5 w-5 shrink-0 animate-spin text-[#d4a574]" />
        ) : (
          <Search className="h-5 w-5 shrink-0 text-[#555]" />
        )}
        <input
          ref={null as unknown as React.RefObject<HTMLInputElement>}
          type="text"
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !loading) onSearch()
          }}
          placeholder="Ask anything..."
          className="flex-1 bg-transparent text-base outline-none placeholder:text-[#555]"
          autoFocus
        />
        <button
          type="button"
          onClick={onSearch}
          disabled={loading || !query.trim()}
          className="shrink-0 rounded-lg bg-[#d4a574] p-2 text-[#111111] hover:bg-[#ebc396] disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          <ArrowRight className="h-4 w-4" />
        </button>
      </div>
    </div>
  )
}

function InitialState({
  history,
  onHistoryClick,
}: {
  history: HistoryEntry[]
  onHistoryClick: (entry: HistoryEntry) => void
}) {
  return (
    <div className="flex flex-col items-center justify-center px-4 py-16">
      <Sparkles className="h-10 w-10 text-[#d4a574] mb-4" />
      <h1 className="text-2xl font-semibold mb-2">AI Search</h1>
      <p className="text-sm text-[#787878] mb-8">Real-time web search with AI-powered answers</p>

      {history.length > 0 && (
        <div className="w-full max-w-2xl">
          <h2 className="mb-3 text-xs font-medium uppercase tracking-wider text-[#555]">Recent Searches</h2>
          <div className="grid gap-1">
            {history.slice(0, 8).map((entry) => (
              <button
                key={`${entry.timestamp}-${entry.query}`}
                type="button"
                onClick={() => onHistoryClick(entry)}
                className="flex items-center gap-2 rounded-lg px-3 py-2 text-left text-sm text-[#b4b4b4] hover:bg-[#1a1a1a] hover:text-[#e8e8e8] transition-colors"
              >
                <Clock className="h-3.5 w-3.5 shrink-0 text-[#555]" />
                <span className="truncate">{entry.query}</span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

function ResultsState({
  phase,
  query,
  answer,
  results,
  questions,
  error,
  onQuestionClick,
  history,
  onHistoryClick,
}: {
  phase: "searching" | "streaming" | "complete"
  query: string
  answer: string
  results: SearchResultItem[]
  questions: string[]
  error: string
  onQuestionClick: (q: string) => void
  history: HistoryEntry[]
  onHistoryClick: (e: HistoryEntry) => void
}) {
  return (
    <div className="mx-auto max-w-3xl px-4 py-6">
      {/* Query display */}
      <h1 className="mb-6 text-xl font-semibold">{query}</h1>

      {/* Loading state */}
      {phase === "searching" && (
        <div className="flex items-center gap-3 py-8 text-[#787878]">
          <Loader2 className="h-5 w-5 animate-spin text-[#d4a574]" />
          <span className="text-sm">Searching the web...</span>
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="rounded-lg border border-[#f44336]/30 bg-[#1a1111] px-4 py-3 text-sm text-[#ffb4b4] mb-4">
          {error}
        </div>
      )}

      {/* AI Answer */}
      {answer && (
        <div className="mb-8">
          <div className="mb-2 flex items-center gap-2 text-xs text-[#787878]">
            <Sparkles className="h-3.5 w-3.5 text-[#d4a574]" />
            <span>AI Answer{phase === "streaming" && " — streaming..."}</span>
          </div>
          <div className="prose prose-invert max-w-none text-sm leading-relaxed">
            <MarkdownContent content={answer} />
          </div>
          {phase === "streaming" && (
            <span className="inline-block h-4 w-0.5 animate-pulse bg-[#d4a574] ml-0.5" />
          )}
        </div>
      )}

      {/* References */}
      {results.length > 0 && phase !== "searching" && (
        <div className="mb-8">
          <h2 className="mb-3 text-xs font-medium uppercase tracking-wider text-[#555]">
            Sources ({results.length})
          </h2>
          <div className="grid gap-2">
            {results.map((result, i) => (
              <SearchResultCard key={result.url} result={result} index={i + 1} />
            ))}
          </div>
        </div>
      )}

      {/* Related Questions */}
      {questions.length > 0 && (
        <div className="mb-8">
          <h2 className="mb-3 text-xs font-medium uppercase tracking-wider text-[#555]">Related</h2>
          <div className="grid gap-1">
            {questions.map((q) => (
              <button
                key={q}
                type="button"
                onClick={() => onQuestionClick(q)}
                className="flex items-center gap-2 rounded-lg px-3 py-2 text-left text-sm text-[#b4b4b4] hover:bg-[#1a1a1a] hover:text-[#e8e8e8] transition-colors"
              >
                <ArrowRight className="h-3.5 w-3.5 shrink-0 text-[#555]" />
                {q}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Search history sidebar hint */}
      {history.length > 0 && (
        <div className="border-t border-[#2a2a2a] pt-4">
          <h2 className="mb-2 text-xs font-medium text-[#555]">Recent searches</h2>
          <div className="flex flex-wrap gap-1.5">
            {history.slice(0, 6).map((entry) => (
              <button
                key={`side-${entry.timestamp}`}
                type="button"
                onClick={() => onHistoryClick(entry)}
                className="rounded-full border border-[#333] px-3 py-1 text-[11px] text-[#787878] hover:border-[#555] hover:text-[#e8e8e8] transition-colors"
              >
                {entry.query.length > 30 ? `${entry.query.slice(0, 30)}...` : entry.query}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
