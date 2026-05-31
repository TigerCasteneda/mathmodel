"use client"

import { useState } from "react"
import { Search, Sparkles, BookOpen, Loader2 } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import { useAgentStatus } from "@/hooks/use-agent-status"
import {
  researchSearch,
  saveResearchItems,
  listResearchItems,
  updateResearchItem,
  deleteResearchItem,
  type SearchResultItem,
  type ResearchItem,
  type SaveItemInput,
} from "@/lib/api"

const SEARCH_TYPES = [
  { value: "literature", label: "📄 Literature" },
  { value: "dataset", label: "📊 Dataset" },
  { value: "code", label: "🧮 Code" },
  { value: "formula", label: "📐 Formula" },
  { value: "competition", label: "🏆 Competition" },
]

const TYPE_BADGES: Record<string, string> = {
  literature: "bg-blue-500/10 text-blue-400",
  dataset: "bg-green-500/10 text-green-400",
  code: "bg-purple-500/10 text-purple-400",
  formula: "bg-amber-500/10 text-amber-400",
  competition: "bg-red-500/10 text-red-400",
}

const TYPE_LABELS: Record<string, string> = {
  literature: "📄 Literature",
  dataset: "📊 Dataset",
  code: "🧮 Code",
  formula: "📐 Formula",
  competition: "🏆 Competition",
}

// ── Search Result Card ──

function ResultCard({
  result,
  selected,
  onToggle,
}: {
  result: SearchResultItem
  selected: boolean
  onToggle: () => void
}) {
  return (
    <div
      className={cn(
        "relative p-4 rounded-lg border transition-all cursor-pointer group",
        "hover:border-primary/40",
        selected
          ? "border-primary/50 bg-primary/5"
          : "border-border bg-card"
      )}
      onClick={onToggle}
    >
      <div className="absolute top-3 right-3">
        <Checkbox checked={selected} onCheckedChange={onToggle} />
      </div>
      <div className="pr-8">
        <h4 className="font-medium text-sm text-foreground line-clamp-2 mb-1">
          {result.title}
        </h4>
        <a
          href={result.url}
          target="_blank"
          rel="noopener noreferrer"
          className="text-xs text-primary/60 hover:text-primary truncate block mb-2"
          onClick={(e) => e.stopPropagation()}
        >
          {result.url}
        </a>
        <p className="text-xs text-muted-foreground line-clamp-3">
          {result.content}
        </p>
      </div>
    </div>
  )
}

// ── Saved Item Card ──

function ItemCard({
  item,
  onDelete,
}: {
  item: ResearchItem
  onDelete: (id: string) => void
}) {
  const [expanded, setExpanded] = useState(false)
  const [notes, setNotes] = useState(item.notes || "")
  const [saving, setSaving] = useState(false)

  const handleSaveNotes = async () => {
    setSaving(true)
    try {
      await updateResearchItem(item.id, { notes })
    } catch (err) {
      console.error("Failed to save notes:", err)
    }
    setSaving(false)
  }

  return (
    <div className="p-4 rounded-lg border border-border bg-card">
      <div className="flex items-start justify-between">
        <div
          className="flex-1 cursor-pointer"
          onClick={() => setExpanded(!expanded)}
        >
          <div className="flex items-center gap-2 mb-1">
            <span
              className={cn(
                "text-xs px-2 py-0.5 rounded-full",
                TYPE_BADGES[item.category] || TYPE_BADGES.literature
              )}
            >
              {TYPE_LABELS[item.category] || item.category}
            </span>
          </div>
          <h4 className="font-medium text-sm text-foreground line-clamp-1">
            {item.title || "Untitled"}
          </h4>
          <p className="text-xs text-muted-foreground mt-1">
            {item.url}
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-muted-foreground hover:text-destructive"
          onClick={() => onDelete(item.id)}
        >
          ✕
        </Button>
      </div>

      {expanded && (
        <div className="mt-3 pt-3 border-t border-border space-y-3">
          {item.summary && (
            <div>
              <span className="text-xs font-medium text-muted-foreground">
                Summary
              </span>
              <p className="text-xs text-muted-foreground mt-1">
                {item.summary}
              </p>
            </div>
          )}
          <div>
            <span className="text-xs font-medium text-muted-foreground">
              Notes
            </span>
            <textarea
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              className="w-full mt-1 p-2 text-xs bg-muted rounded border border-border text-foreground resize-none h-20"
              placeholder="Add your notes..."
            />
            <Button
              size="sm"
              variant="outline"
              className="mt-2 h-7 text-xs"
              onClick={handleSaveNotes}
              disabled={saving}
            >
              {saving ? "Saving..." : "Save Notes"}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}

// ── Main Component ──

export function MainWorkspace({ projectId }: { projectId: string }) {
  const agentStatus = useAgentStatus(projectId)

  // Search state
  const [activeTab, setActiveTab] = useState<"search" | "library">("search")
  const [query, setQuery] = useState("")
  const [category, setCategory] = useState("literature")
  const [results, setResults] = useState<SearchResultItem[]>([])
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [isSearching, setIsSearching] = useState(false)
  const [hasSearched, setHasSearched] = useState(false)
  const [isSaving, setIsSaving] = useState(false)

  // Library state
  const [items, setItems] = useState<ResearchItem[]>([])
  const [libraryLoaded, setLibraryLoaded] = useState(false)

  const loadLibrary = async () => {
    try {
      const data = await listResearchItems(projectId)
      setItems(data)
      setLibraryLoaded(true)
    } catch (err) {
      console.error("Failed to load research items:", err)
    }
  }

  const handleSearch = async () => {
    if (!query.trim()) return
    setIsSearching(true)
    setHasSearched(false)
    setSelected(new Set())
    try {
      const resp = await researchSearch(projectId, query, category)
      setResults(resp.results)
    } catch (err) {
      console.error("Search failed:", err)
      setResults([])
    }
    setIsSearching(false)
    setHasSearched(true)
  }

  const toggleResult = (index: number) => {
    const next = new Set(selected)
    if (next.has(index)) next.delete(index)
    else next.add(index)
    setSelected(next)
  }

  const handleSave = async () => {
    const toSave: SaveItemInput[] = []
    selected.forEach((i) => {
      const r = results[i]
      if (r) {
        toSave.push({
          title: r.title,
          url: r.url,
          content: r.content,
          category,
          summary: r.content.slice(0, 500),
          relevance_score: r.relevance_score,
          raw_json: r as unknown as Record<string, unknown>,
        })
      }
    })
    if (toSave.length === 0) return

    setIsSaving(true)
    try {
      const resp = await saveResearchItems(projectId, toSave)
      alert(`Saved ${resp.saved} item(s).${resp.files_created < resp.saved ? " Agent not connected — local files not created." : ""}`)
      setSelected(new Set())
    } catch (err) {
      console.error("Save failed:", err)
      alert("Failed to save items.")
    }
    setIsSaving(false)
  }

  const handleDelete = async (itemId: string) => {
    if (!confirm("Delete this research item?")) return
    try {
      await deleteResearchItem(itemId)
      setItems((prev) => prev.filter((it) => it.id !== itemId))
    } catch (err) {
      console.error("Delete failed:", err)
    }
  }

  const selectedCount = selected.size

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Tab Switcher */}
      <div className="flex border-b border-border">
        <button
          className={cn(
            "flex-1 py-3 text-sm font-medium flex items-center justify-center gap-2 transition-colors",
            activeTab === "search"
              ? "text-foreground border-b-2 border-primary"
              : "text-muted-foreground hover:text-foreground"
          )}
          onClick={() => setActiveTab("search")}
        >
          <Search className="w-4 h-4" />
          Search
        </button>
        <button
          className={cn(
            "flex-1 py-3 text-sm font-medium flex items-center justify-center gap-2 transition-colors",
            activeTab === "library"
              ? "text-foreground border-b-2 border-primary"
              : "text-muted-foreground hover:text-foreground"
          )}
          onClick={() => {
            setActiveTab("library")
            if (!libraryLoaded) loadLibrary()
          }}
        >
          <BookOpen className="w-4 h-4" />
          Research Library
        </button>
      </div>

      {/* ── Search Tab ── */}
      {activeTab === "search" && (
        <>
          {/* Search Bar */}
          <div className="p-4 border-b border-border">
            <div className="flex gap-2 mb-2">
              <select
                value={category}
                onChange={(e) => setCategory(e.target.value)}
                className="px-3 py-2 text-sm bg-muted border border-border rounded-lg text-foreground"
              >
                {SEARCH_TYPES.map((t) => (
                  <option key={t.value} value={t.value}>
                    {t.label}
                  </option>
                ))}
              </select>
              <div className="flex-1 relative">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
                <Input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleSearch()}
                  placeholder="Search for mathematical models, methods, papers..."
                  className="pl-10 pr-4 py-5 text-sm bg-input border-border rounded-xl"
                />
              </div>
              <Button
                onClick={handleSearch}
                disabled={isSearching || !query.trim()}
                className="bg-primary text-primary-foreground hover:bg-primary/90"
              >
                {isSearching ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Sparkles className="w-4 h-4" />
                )}
              </Button>
            </div>
          </div>

          {/* Results */}
          <ScrollArea className="flex-1">
            <div className="p-4 max-w-4xl mx-auto">
              {isSearching ? (
                <div className="space-y-4">
                  {[1, 2, 3, 4].map((i) => (
                    <div key={i} className="p-4 rounded-lg border border-border">
                      <Skeleton className="h-4 w-3/4 bg-muted mb-2" />
                      <Skeleton className="h-3 w-1/2 bg-muted mb-3" />
                      <Skeleton className="h-3 w-full bg-muted" />
                    </div>
                  ))}
                </div>
              ) : hasSearched && results.length === 0 ? (
                <div className="text-center py-20 text-muted-foreground">
                  <p>No results found. Try a different query.</p>
                </div>
              ) : hasSearched ? (
                <div className="grid grid-cols-1 gap-3">
                  {results.map((r, i) => (
                    <ResultCard
                      key={i}
                      result={r}
                      selected={selected.has(i)}
                      onToggle={() => toggleResult(i)}
                    />
                  ))}
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center py-20 text-center">
                  <div className="w-16 h-16 rounded-2xl bg-primary/10 flex items-center justify-center mb-4">
                    <Sparkles className="w-8 h-8 text-primary" />
                  </div>
                  <h3 className="text-lg font-medium text-foreground mb-2">
                    Start Your Research
                  </h3>
                  <p className="text-sm text-muted-foreground max-w-sm">
                    Search for mathematical models, datasets, code examples, formulas, and competition papers.
                  </p>
                </div>
              )}
            </div>
          </ScrollArea>

          {/* Save Bar */}
          {selectedCount > 0 && (
            <div className="sticky bottom-0 p-4 border-t border-border bg-background/80 backdrop-blur-sm">
              <div className="flex items-center justify-between max-w-4xl mx-auto">
                <span className="text-sm text-muted-foreground">
                  {selectedCount} selected
                  {agentStatus !== "ready" && (
                    <span className="text-amber-400 ml-2">
                      ⚠ Agent offline — cloud save only
                    </span>
                  )}
                </span>
                <Button
                  onClick={handleSave}
                  disabled={isSaving}
                  className="bg-primary text-primary-foreground hover:bg-primary/90"
                >
                  {isSaving ? (
                    <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                  ) : (
                    <Sparkles className="w-4 h-4 mr-2" />
                  )}
                  Save {selectedCount} Item{selectedCount > 1 ? "s" : ""}
                </Button>
              </div>
            </div>
          )}
        </>
      )}

      {/* ── Research Library Tab ── */}
      {activeTab === "library" && (
        <ScrollArea className="flex-1">
          <div className="p-4 max-w-4xl mx-auto">
            {items.length === 0 ? (
              <div className="text-center py-20 text-muted-foreground">
                <BookOpen className="w-12 h-12 mx-auto mb-4 opacity-30" />
                <p>No saved research items yet.</p>
                <p className="text-sm mt-1">
                  Switch to the Search tab to find and save references.
                </p>
              </div>
            ) : (
              <div className="space-y-3">
                {/* Filter */}
                <div className="flex gap-2 mb-4">
                  <select
                    className="px-3 py-1.5 text-xs bg-muted border border-border rounded-lg text-foreground"
                    onChange={(e) => {
                      if (!e.target.value) {
                        loadLibrary()
                      } else {
                        listResearchItems(projectId, e.target.value).then(setItems).catch(console.error)
                      }
                    }}
                  >
                    <option value="">All Types</option>
                    {SEARCH_TYPES.map((t) => (
                      <option key={t.value} value={t.value}>
                        {t.label}
                      </option>
                    ))}
                  </select>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-xs"
                    onClick={loadLibrary}
                  >
                    Refresh
                  </Button>
                </div>
                {items.map((item) => (
                  <ItemCard
                    key={item.id}
                    item={item}
                    onDelete={handleDelete}
                  />
                ))}
              </div>
            )}
          </div>
        </ScrollArea>
      )}
    </div>
  )
}
