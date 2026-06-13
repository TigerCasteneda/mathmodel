"use client"

import { useState } from "react"
import { Loader2, Send } from "lucide-react"
import { cn } from "@/lib/utils"
import type { ResearchItem } from "@/lib/api"
import type { ArenaCardType } from "@/lib/research-to-arena"

const CARD_TYPES: ArenaCardType[] = ["formula", "finding", "assumption", "decision", "note"]

// One saved research reference, with a card-type picker and a Send-to-Arena
// action. The picker defaults to the category-mapped type but lets the user
// override before sending.
export function KnowledgeResearchRow({
  item,
  canWrite,
  sending,
  defaultCardType,
  onSend,
}: {
  item: ResearchItem
  canWrite: boolean
  sending: boolean
  defaultCardType: ArenaCardType
  onSend: (cardType: ArenaCardType) => void
}) {
  const [cardType, setCardType] = useState<ArenaCardType>(defaultCardType)

  return (
    <div className="rounded-md border border-[#2a2a2a] bg-[#141414] px-3 py-2">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="rounded-full border border-[#373737] px-1.5 py-0.5 text-[10px] uppercase text-[#d4a574]">{item.category}</span>
            <span className="min-w-0 flex-1 truncate text-xs text-[#e8e8e8]">{item.title || "Untitled"}</span>
          </div>
          {item.summary && (
            <p className="mt-1 line-clamp-2 text-[11px] leading-4 text-[#787878]">{item.summary}</p>
          )}
        </div>
      </div>

      <div className="mt-2 flex items-center gap-1.5">
        <select
          value={cardType}
          onChange={(event) => setCardType(event.target.value as ArenaCardType)}
          disabled={!canWrite || sending}
          className="rounded-md border border-[#373737] bg-[#1a1a1a] px-1.5 py-1 text-[11px] text-[#b4b4b4] disabled:opacity-50"
        >
          {CARD_TYPES.map((type) => (
            <option key={type} value={type}>{type}</option>
          ))}
        </select>
        <button
          type="button"
          onClick={() => onSend(cardType)}
          disabled={!canWrite || sending}
          title={canWrite ? "Send to Arena" : "files.write permission required"}
          className={cn(
            "inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-[11px] transition-colors",
            canWrite
              ? "border-[#d4a574]/40 text-[#ebc396] hover:border-[#d4a574] hover:bg-[#2d241a]"
              : "border-[#373737] text-[#5f5f5f] cursor-not-allowed",
          )}
        >
          {sending ? <Loader2 className="h-3 w-3 animate-spin" /> : <Send className="h-3 w-3" />}
          Send to Arena
        </button>
      </div>
    </div>
  )
}
