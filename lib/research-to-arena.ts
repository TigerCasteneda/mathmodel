import type { CreateArenaCardInput, ResearchItem } from "@/lib/api"
import type { NativeResearchSearchItem } from "@/lib/tauri-api"

// Bridge: turn research findings into Arena cards via direct field mapping
// (no AI call). Both `ResearchItem` (saved) and `NativeResearchSearchItem`
// (fresh search result) collapse onto `CreateArenaCardInput`.

export type ArenaCardType = "formula" | "finding" | "assumption" | "decision" | "note"

const CATEGORY_TO_CARD_TYPE: Record<string, ArenaCardType> = {
  literature: "finding",
  paper: "finding",
  method: "formula",
  algorithm: "formula",
  dataset: "note",
  code: "note",
  docs: "note",
}

// Best-effort category → card type, defaulting to "finding".
export function cardTypeForCategory(category?: string | null): ArenaCardType {
  if (!category) return "finding"
  return CATEGORY_TO_CARD_TYPE[category.trim().toLowerCase()] || "finding"
}

function buildTags(category: string | undefined, keywords?: string | null): string[] {
  const tags = ["research"]
  if (category?.trim()) tags.push(category.trim().toLowerCase())
  if (keywords?.trim()) {
    for (const keyword of keywords.split(",")) {
      const cleaned = keyword.trim().toLowerCase()
      if (cleaned) tags.push(cleaned)
    }
  }
  // Dedupe while preserving order.
  return Array.from(new Set(tags))
}

type BodySection = { heading: string; value?: string | null }

// Assemble a markdown body from whatever fields are present, skipping empties.
function buildBody(title: string, url: string | undefined, sections: BodySection[]): string {
  const lines: string[] = [`# ${title}`, ""]
  if (url?.trim()) {
    lines.push(`> Source: [${url}](${url})`, "")
  }
  for (const { heading, value } of sections) {
    if (value?.trim()) {
      lines.push(`## ${heading}`, value.trim(), "")
    }
  }
  return lines.join("\n")
}

// Saved research item → Arena card input.
export function researchItemToArenaInput(
  item: ResearchItem,
  cardTypeOverride?: ArenaCardType,
): CreateArenaCardInput {
  const title = item.title?.trim() || "Untitled Reference"
  return {
    card_type: cardTypeOverride || cardTypeForCategory(item.category),
    title,
    tags: buildTags(item.category, item.keywords),
    body: buildBody(title, item.url, [
      { heading: "Summary", value: item.summary },
      { heading: "Methodology", value: item.methodology },
      { heading: "Key Parameters", value: item.key_parameters },
      { heading: "AI Relevance", value: item.ai_relevance },
      { heading: "Notes", value: item.notes },
    ]),
  }
}

// Fresh (unsaved) search result → Arena card input. Has fewer fields; uses
// `content` as the summary and `reason` as relevance.
export function searchResultToArenaInput(
  item: NativeResearchSearchItem,
  cardTypeOverride?: ArenaCardType,
): CreateArenaCardInput {
  const title = item.title?.trim() || "Untitled Reference"
  return {
    card_type: cardTypeOverride || cardTypeForCategory(item.category),
    title,
    tags: buildTags(item.category, item.keywords),
    body: buildBody(title, item.url, [
      { heading: "Summary", value: item.content },
      { heading: "Relevance", value: item.reason },
    ]),
  }
}
