"use client"

import { BookOpen, Database, ExternalLink, FileCode, Globe2, Loader2, Search } from "lucide-react"
import { type AgentSource } from "@/lib/tauri-api"

type Phase = "initial" | "running" | "complete"

// A tool invocation as tracked in the timeline (keyed by tool-call id).
interface ToolEntry {
  id: string
  name: string
  arguments: Record<string, unknown>
  status: "running" | "success" | "error"
  summary: string
}

const TOOL_ICON: Record<string, typeof Search> = {
  search_academic: BookOpen,
  search_web: Globe2,
  fetch_url: ExternalLink,
}

function toolLabel(entry: ToolEntry): string {
  const q = (entry.arguments.query as string) || (entry.arguments.url as string) || ""
  const kind = entry.arguments.kind as string | undefined
  switch (entry.name) {
    case "search_academic":
      return `Searched ${kind ?? "academic"} · ${q}`
    case "search_web":
      return `Web search · ${q}`
    case "fetch_url":
      return `Read ${q}`
    default:
      return entry.name
  }
}

function ToolCard({ entry }: { entry: ToolEntry }) {
  const Icon = TOOL_ICON[entry.name] ?? Search
  return (
    <div className="flex items-center gap-2 rounded-md border border-[#373737] bg-[#1a1a1a] px-3 py-2 text-xs">
      <Icon className="h-3.5 w-3.5 shrink-0 text-[#d4a574]" />
      <span className="min-w-0 flex-1 truncate text-[#b4b4b4]">{toolLabel(entry)}</span>
      {entry.status === "running" ? (
        <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-[#d4a574]" />
      ) : entry.status === "error" ? (
        <span className="shrink-0 text-[10px] text-[#d49a9a]">{entry.summary || "error"}</span>
      ) : (
        <span className="shrink-0 text-[10px] text-[#787878]">{entry.summary}</span>
      )}
    </div>
  )
}

const SOURCE_ICON: Record<string, typeof BookOpen> = {
  literature: BookOpen,
  dataset: Database,
  code: FileCode,
}

function SourceCard({
  source,
  selected,
  onToggle,
}: {
  source: AgentSource
  selected: boolean
  onToggle: () => void
}) {
  const Icon = SOURCE_ICON[source.category] ?? BookOpen
  return (
    <div
      id={`ref-${source.citation}`}
      className="rounded-lg border border-[#373737] bg-[#0d0d0d] px-3 py-2.5"
    >
      <div className="flex items-start gap-2">
        <input
          type="checkbox"
          checked={selected}
          onChange={onToggle}
          className="mt-1 h-3.5 w-3.5 shrink-0 accent-[#d4a574]"
        />
        <span className="mt-0.5 shrink-0 text-[11px] font-medium text-[#555]">[{source.citation}]</span>
        <div className="min-w-0 flex-1">
          <a
            href={source.url}
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm font-medium text-[#d4a574] underline underline-offset-2"
          >
            {source.title}
          </a>
          <div className="mt-0.5 flex items-center gap-1.5">
            <Icon className="h-3 w-3 text-[#555]" />
            <span className="text-[10px] uppercase text-[#787878]">{source.provider}</span>
          </div>
          <p className="mt-1 line-clamp-2 text-xs leading-relaxed text-[#b4b4b4]">{source.content}</p>
        </div>
        <a
          href={source.url}
          target="_blank"
          rel="noopener noreferrer"
          className="ml-auto shrink-0 rounded p-1 text-[#555] hover:bg-[#1a1a1a] hover:text-[#e8e8e8]"
        >
          <ExternalLink className="h-3.5 w-3.5" />
        </a>
      </div>
    </div>
  )
}

export { ToolCard, SourceCard }
export type { ToolEntry, Phase }
