"use client"

import { useCallback, useEffect, useMemo, useState } from "react"
import { BookOpen, Loader2, Network, RefreshCw, Search } from "lucide-react"
import { Input } from "@/components/ui/input"
import { cn } from "@/lib/utils"
import {
  createArenaCard,
  getArenaIndex,
  listResearchItems,
  type ArenaCard,
  type ResearchItem,
} from "@/lib/api"
import {
  cardTypeForCategory,
  researchItemToArenaInput,
  type ArenaCardType,
} from "@/lib/research-to-arena"
import { KnowledgeResearchRow } from "@/components/knowledge/knowledge-research-row"

const CARD_TYPE_TONE: Record<string, string> = {
  formula: "border-[#64b5f6] text-[#9fd0ff]",
  finding: "border-[#9bd6b5] text-[#b6edcb]",
  assumption: "border-[#d4a574] text-[#ebc396]",
  decision: "border-[#f87171] text-[#ffb4a8]",
  note: "border-[#9b8cff] text-[#c7bdff]",
}

function matchesQuery(query: string, ...fields: Array<string | undefined>) {
  if (!query.trim()) return true
  const needle = query.trim().toLowerCase()
  return fields.some((field) => field?.toLowerCase().includes(needle))
}

export function KnowledgeBasePanel({
  projectId,
  capabilities = [],
}: {
  projectId: string
  capabilities?: string[]
}) {
  const [cards, setCards] = useState<ArenaCard[]>([])
  const [items, setItems] = useState<ResearchItem[]>([])
  const [query, setQuery] = useState("")
  const [loading, setLoading] = useState(false)
  const [sendingId, setSendingId] = useState<string | null>(null)
  const [status, setStatus] = useState<string | null>(null)

  const canWrite = capabilities.includes("files.write")

  const refreshCards = useCallback(async () => {
    try {
      const index = await getArenaIndex(projectId)
      setCards(index.cards)
    } catch {
      setCards([])
    }
  }, [projectId])

  const refreshAll = useCallback(async () => {
    setLoading(true)
    setStatus(null)
    try {
      const [index, research] = await Promise.all([
        getArenaIndex(projectId),
        listResearchItems(projectId).catch(() => [] as ResearchItem[]),
      ])
      setCards(index.cards)
      setItems(research)
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Knowledge base unavailable.")
    } finally {
      setLoading(false)
    }
  }, [projectId])

  useEffect(() => {
    void refreshAll()
  }, [refreshAll])

  const filteredCards = useMemo(
    () => cards.filter((card) => matchesQuery(query, card.title, card.tags.join(" "), card.card_type)),
    [cards, query],
  )
  const filteredItems = useMemo(
    () => items.filter((item) => matchesQuery(query, item.title, item.keywords, item.category, item.summary)),
    [items, query],
  )

  const sendToArena = async (item: ResearchItem, cardType: ArenaCardType) => {
    if (!canWrite || sendingId) return
    setSendingId(item.id)
    setStatus(null)
    try {
      const input = researchItemToArenaInput(item, cardType)
      await createArenaCard(projectId, input)
      setStatus(`Added to Arena: ${input.title}`)
      await refreshCards()
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Send to Arena failed.")
    } finally {
      setSendingId(null)
    }
  }

  return (
    <section className="flex h-full min-h-0 flex-col bg-[#0d0d0d] text-[#e8e8e8]">
      {/* Header */}
      <div className="flex h-11 items-center gap-2 border-b border-[#373737] bg-[#121212] px-3 shrink-0">
        <Network className="h-4 w-4 text-[#d4a574]" />
        <span className="text-sm font-medium">Knowledge Base</span>
        <div className="relative ml-3 flex-1 max-w-md">
          <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[#787878]" />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter cards and references..."
            className="h-8 border-[#373737] bg-[#1a1a1a] pl-8 text-xs"
          />
        </div>
        <button
          onClick={() => refreshAll()}
          title="Refresh"
          className="ml-auto text-[#787878] hover:text-[#e8e8e8]"
        >
          <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
        </button>
      </div>

      {status && (
        <div className="border-b border-[#373737] bg-[#181818] px-3 py-2 text-xs text-[#b4b4b4]">{status}</div>
      )}

      {/* Two columns */}
      <div className="grid min-h-0 flex-1 grid-cols-2 divide-x divide-[#373737]">
        {/* Arena cards */}
        <div className="flex min-h-0 flex-col">
          <div className="flex items-center gap-2 border-b border-[#373737] px-3 py-2 text-xs font-medium uppercase tracking-wide text-[#b4b4b4]">
            <Network className="h-3.5 w-3.5 text-[#d4a574]" />
            Arena Cards
            <span className="ml-auto rounded-full bg-[#1a1a1a] px-2 py-0.5 text-[10px] text-[#787878]">{filteredCards.length}</span>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto p-2">
            {filteredCards.length === 0 ? (
              <div className="px-3 py-8 text-center text-xs text-[#787878]">
                {loading ? "Loading..." : "No cards."}
              </div>
            ) : (
              <div className="grid gap-1.5">
                {filteredCards.map((card) => (
                  <div key={card.file_id} className="rounded-md border border-[#2a2a2a] bg-[#141414] px-3 py-2">
                    <div className="flex items-center gap-2">
                      <span className={cn("rounded border px-1.5 py-0.5 text-[10px]", CARD_TYPE_TONE[card.card_type] || "border-[#787878] text-[#b4b4b4]")}>{card.card_type}</span>
                      <span className="min-w-0 flex-1 truncate text-xs text-[#e8e8e8]">{card.title}</span>
                    </div>
                    {card.tags.length > 0 && (
                      <div className="mt-1 truncate text-[11px] text-[#787878]">{card.tags.map((tag) => `#${tag}`).join(" ")}</div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* Research items */}
        <div className="flex min-h-0 flex-col">
          <div className="flex items-center gap-2 border-b border-[#373737] px-3 py-2 text-xs font-medium uppercase tracking-wide text-[#b4b4b4]">
            <BookOpen className="h-3.5 w-3.5 text-[#d4a574]" />
            Research
            <span className="ml-auto rounded-full bg-[#1a1a1a] px-2 py-0.5 text-[10px] text-[#787878]">{filteredItems.length}</span>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto p-2">
            {filteredItems.length === 0 ? (
              <div className="px-3 py-8 text-center text-xs text-[#787878]">
                {loading ? "Loading..." : "No saved references."}
              </div>
            ) : (
              <div className="grid gap-1.5">
                {filteredItems.map((item) => (
                  <KnowledgeResearchRow
                    key={item.id}
                    item={item}
                    canWrite={canWrite}
                    sending={sendingId === item.id}
                    defaultCardType={cardTypeForCategory(item.category)}
                    onSend={(cardType) => sendToArena(item, cardType)}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </section>
  )
}
