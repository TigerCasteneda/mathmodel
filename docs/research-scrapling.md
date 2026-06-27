# Agentic Research — Scrapling Backend

## What changed

The `ResearchScraper` enum and the default `SCRAPER_OPTIONS` UI selection now
have **`Scrapling` as the primary** extraction backend. `Firecrawl` and
`Tavily` remain available as configured fallbacks.

```
                 user query
                     │
                     ▼
            ┌────────────────┐
            │ search_web     │  /search/web (DDG via StealthyFetcher)
            │ tool_fetch_url │  /fetch (StealthyFetcher → Selector)
            │ extract_       │  /extract (Selector + CSS hints, or heuristics)
            │ structured     │
            └────────────────┘
                     │ on error / 0 results
                     ▼
         Firecrawl  →  Tavily  →  (return whatever we have)
```

## Why Scrapling

| Capability            | Scrapling                              | Tavily/Firecrawl       |
|-----------------------|----------------------------------------|------------------------|
| API key required      | No                                     | Yes                    |
| Anti-bot bypass       | StealthyFetcher (Cloudflare solver)    | Built-in               |
| Structured extraction | CSS / XPath selectors (via `Selector`) | LLM-driven schema      |
| Search                | DDG HTML scraping (no API key)         | Native search API      |
| Cost                  | Free (local compute)                   | Per-credit             |

## What the new `extract_structured` tool does

The LLM can now call:

```json
{
  "url": "https://arxiv.org/abs/2401.00001",
  "selector_hints": {
    "title": "h1.title::text",
    "authors": ".authors a::text",
    "abstract": "blockquote.abstract::text"
  }
}
```

The sidecar fetches the URL via `StealthyFetcher`, parses the page with
`Selector`, and returns a JSON object:

```json
{
  "url": "https://arxiv.org/abs/2401.00001",
  "fields": {
    "title": "Graph Neural Networks for Traffic Forecasting",
    "authors": "Alice, Bob, Carol",
    "abstract": "We propose ..."
  }
}
```

The Rust side then emits a `research_agent:source_update` event with
`citation` and `structured_data`, and the UI renders a collapsible table on
the corresponding `SourceCard`.

If `selector_hints` is omitted and only `fields` is provided, the sidecar
falls back to id/class heuristics (`#title`, `.authors`, `meta[name=...]`)
and always includes a `_text` field with the page's main text for the model
to parse itself.

## Fallback chain

When the primary returns an error or 0 results, the agent moves through the
configured chain:

| Primary    | Tries first | Tries second | Last resort           |
|------------|-------------|--------------|------------------------|
| Scrapling  | Firecrawl   | Tavily       | Scrapling (self-retry) |
| Firecrawl  | Tavily      | Scrapling    | —                      |
| Tavily     | Firecrawl   | Scrapling    | —                      |

`Scrapling` is the only one that doesn't need an API key, so a user with
neither Firecrawl nor Tavily configured can still get a working
`search_web` path through Scrapling (subject to network reachability — DDG
is blocked in some corporate environments; in that case the agent surfaces
an empty result and the user can configure an API key).

## Sidecar endpoints (added/changed)

| Method | Path             | Purpose                          |
|--------|------------------|----------------------------------|
| POST   | `/search/web`    | DDG HTML scrape, result list     |
| POST   | `/fetch`         | StealthyFetcher → clean text     |
| POST   | `/extract`       | Selector-based field extraction  |
| POST   | `/search/papers` | (legacy) arXiv + S2 + OpenAlex APIs |
| POST   | `/search/datasets`| (legacy) Zenodo + Kaggle + GH APIs |
| POST   | `/search/code`   | (legacy) GitHub API              |
| POST   | `/search/papers/scrapling/arxiv`        | (Phase 8) arxiv.org HTML via `Fetcher` |
| POST   | `/search/papers/scrapling/pubmed`       | (Phase 8) pubmed.ncbi.nlm.nih.gov HTML via `Fetcher` |
| POST   | `/search/papers/scrapling/semanticscholar` | (Phase 8) semanticscholar.org HTML via `StealthyFetcher` |
| POST   | `/search/datasets/scrapling/zenodo`     | (Phase 8) zenodo.org HTML via `Fetcher` |
| POST   | `/search/datasets/scrapling/kaggle`     | (Phase 8) kaggle.com HTML via `StealthyFetcher` |
| POST   | `/search/code/scrapling/github`         | (Phase 8) github.com HTML via `StealthyFetcher` |
| POST   | `/enrich`        | (unchanged) basic URL enrichment |
| GET    | `/health`        | (unchanged) liveness             |

## Academic search: Scrapling as PRIMARY (Phase 8)

For `Paper`/`Dataset`/`Code` searches, the agent now uses **Scrapling HTML
scraping as the PRIMARY path**, with the existing REST APIs as the
**FALLBACK**:

### Paper kind
1. `POST /search/papers/scrapling/arxiv` — arxiv.org HTML via `Fetcher`
2. `POST /search/papers/scrapling/pubmed` — PubMed HTML via `Fetcher`
3. `POST /search/papers/scrapling/semanticscholar` — S2 HTML via `StealthyFetcher` (needs Chromium)
4. `POST /search/papers` — legacy sidecar (arxiv/S2/OpenAlex REST APIs)
5. Firecrawl or Tavily (if API key configured)

### Dataset kind
1. `POST /search/datasets/scrapling/zenodo` — zenodo.org HTML via `Fetcher`
2. `POST /search/datasets/scrapling/kaggle` — kaggle.com HTML via `StealthyFetcher` (needs Chromium)
3. `POST /search/datasets` — legacy sidecar (Zenodo/Kaggle/GitHub REST APIs)
4. Firecrawl or Tavily

### Code kind
1. `POST /search/code/scrapling/github` — github.com HTML via `StealthyFetcher` (needs Chromium)
2. `POST /search/code` — legacy sidecar (GitHub REST API)
3. Firecrawl or Tavily

Scrapling scrapers are tried in order; each one is awaited before the next.
If a scraper returns 0 items we move on to the next Scrapling source.
Once we've exhausted Scrapling we fall through to the API sidecar to top
up to `limit`; if the API returns 0 we finish with the configured API-key
scraper (Firecrawl/Tavily). If Scrapling gives us a partial set the result
includes a `warning` string listing the tried Scrapling sources.

### Why HTML scraping first (not the API)

- **No API keys** required. arxiv.org, pubmed.ncbi.nlm.nih.gov, and
  zenodo.org HTML search pages are public.
- **Survives API outages** — if the arxiv API or S2 API is down, the
  HTML search still works.
- **Same pattern** as the DDG web search (`/search/web`) — consistency
  for operators.
- **Sites that need Chromium**: semanticscholar.org, github.com
  (anti-bot), and kaggle.com (JS rendering). These return 0 without
  Chromium installed; the API fallback kicks in transparently.
- **Replaces** the pre-Phase-8 flow where the API sidecar was tried
  first and Scrapling was only the last-resort fallback. The user
  explicitly wanted Scrapling as the primary academic backend.

### Implementation reference

The Rust dispatch lives in `src-tauri/src/ai/research.rs`:

- `search_academic_with_scrapling_primary(sidecar, config, query, kind, limit)`
  is the new primary entry. It calls each HTML scraper in priority
  order (per `kind`), accumulates items, then falls through to
  `search_sidecar` (API) and finally `search_academic_api_fallback`
  (Firecrawl/Tavily).
- `research_search_for_agent(...)` is the agent-facing entry; for
  academic kinds with the sidecar enabled it routes through the
  Scrapling-primary function. For Web/Auto kinds it still uses the
  DDG-backed `search_scrapling` path. Docs still use Context7.

## Setup

The sidecar now requires `scrapling[fetchers]>=0.4.0` as a **required**
runtime dep (it was optional before). Install with:

```bash
cd src-tauri/sidecar
py -3 -m pip install -e .
```

If you want the stealthy browser backend (used by `StealthyFetcher.fetch`)
you also need to install the Chromium binary:

```bash
py -3 -m playwright install chromium
```

Without Chromium, `StealthyFetcher` will fail and the sidecar will return
errors from `/fetch` and `/search/web`. The plain `Fetcher` (curl_cffi)
path still works for `/fetch`'s plain GETs but does not bypass anti-bot.

## Testing

- Rust unit tests: `cargo test --lib -p modeler-desktop` — 90 tests pass.
- Sidecar unit tests: `cd src-tauri/sidecar && py -3 -m pytest tests/ -v`
  — 10 tests pass.
- TypeScript: `pnpm exec tsc --noEmit` — clean.

## Files touched

### Backend (Rust)
- `src/ai/research.rs` — `Scrapling` enum variant, `search_scrapling`,
  `fetch_url_scrapling`, updated `dispatch_scraper_search` +
  `pick_fallback_scraper` + `search_with_fallback` signatures
- `src/ai/research_agent.rs` — `extract_structured` tool, new event
  `AgentSourceUpdateEvent`, `tool_fetch_url` reuses the sidecar
- `src/ai/history.rs` — `classify_operation("extract_structured")` →
  `OperationType::FetchUrl`

### Sidecar (Python)
- `sidecar/app/providers/stealth.py` — `fetch_page`, `extract`,
  `search_web`, `_normalise_ddg_url` (DDG redirect unwrap)
- `sidecar/app/main.py` — new routes `/search/web`, `/fetch`, `/extract`
- `sidecar/app/models.py` — `FetchRequest/Response`,
  `ExtractRequest/Response`
- `sidecar/pyproject.toml` — `scrapling[fetchers]` is now required
- `sidecar/tests/test_stealth.py` — 10 unit tests covering fetch, extract,
  search, DDG URL unwrap, error paths

### UI
- `lib/tauri-api.ts` — `ResearchScraper` extended, `AgentSource.structured_data`
  optional field, `AgentSourceUpdateEvent` + `onResearchAgentSourceUpdate`
- `components/research/agent-research-view.tsx` — `update_source` reducer
  action, event subscription
- `components/research/agent-cards.tsx` — `Braces` icon for
  `extract_structured`, collapsible structured-data table on `SourceCard`
- `components/layout/modeler-workbench.tsx` — `SCRAPER_OPTIONS` includes
  Scrapling first; default selection flipped to `"scrapling"`

## Limitations / known gaps

1. **No `Fetcher.auto_match`** — Scrapling 0.4.x removed the AI-driven schema
   matcher. The `extract` endpoint uses CSS selectors / id-class heuristics
   instead. For pages without obvious selectors, the LLM should pass
   `selector_hints` based on prior knowledge of the site, or fall back to
   `fetch_url` which returns the full text.

2. **DDG may be network-blocked** — verified in the dev environment
   (`html.duckduckgo.com:443` unreachable). The fallback chain handles this
   transparently; configure Firecrawl or Tavily to get results when DDG is
   unreachable.

3. **No Chromium in CI / dev by default** — `StealthyFetcher.fetch` will
   fail until `playwright install chromium` runs once. The plain `Fetcher`
   path (`asyncio.to_thread(Fetcher.get, ...)`) works for trusted sites.

4. **HTML→markdown is text-only** — Scrapling's `Selector.get_all_text`
   produces plain text, not real markdown. This is fine for LLM
   consumption (the model doesn't care), but the field is still called
   `markdown` for backward compat with the existing `FetchResponse` shape.
