"""Academic search providers for the sidecar.

Each module exposes a `search(query, limit)` async function returning
`list[SearchResultItem]`. The Rust sidecar client (`src-tauri/src/ai/research.rs`)
calls these via HTTP.

Modules:
- arxiv, semantic_scholar, openalex: paper APIs (REST)
- zenodo, kaggle, github: dataset/code APIs (REST)
- stealth: URL enrichment (Scrapling)
- scrapling_search: HTML-scraping fallback for all kinds (Scrapling, PRIMARY)
"""
