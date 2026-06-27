// Type-level test for `lib/tauri-api.ts` Scrapling integration.
// This file is picked up by `tsc --noEmit` (when configured) and fails
// at compile time if:
//   - `ResearchScraper` does not include the "scrapling" literal
//   - `AgentSource` does not accept the new `structured_data` field
//
// Add this to `tsconfig.json` `include` if it isn't already covered.

import type {
  AgentSource,
  AgentSourceUpdateEvent,
  ResearchScraper,
} from "./tauri-api"

// Compile-time check: each string literal must be assignable to ResearchScraper.
const _scrapling: ResearchScraper = "scrapling"
const _firecrawl: ResearchScraper = "firecrawl"
const _tavily: ResearchScraper = "tavily"

// Compile-time check: AgentSource accepts the new structured_data field,
// with three possible states: missing, null, and populated object.
const _srcMissing: AgentSource = {
  citation: 1,
  title: "x",
  url: "https://x",
  content: "x",
  provider: "scrapling",
  category: "web",
}
const _srcNull: AgentSource = {
  citation: 1,
  title: "x",
  url: "https://x",
  content: "x",
  provider: "scrapling",
  category: "web",
  structured_data: null,
}
const _srcPopulated: AgentSource = {
  citation: 1,
  title: "x",
  url: "https://x",
  content: "x",
  provider: "scrapling",
  category: "web",
  structured_data: { authors: "Alice, Bob", year: 2024 },
}

// Compile-time check: AgentSourceUpdateEvent is well-formed.
const _evt: AgentSourceUpdateEvent = {
  request_id: "r",
  citation: 1,
  structured_data: { a: 1 },
}

// Use the bindings to silence "unused" warnings.
void _scrapling
void _firecrawl
void _tavily
void _srcMissing
void _srcNull
void _srcPopulated
void _evt