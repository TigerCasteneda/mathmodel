import asyncio
import logging
import time
import traceback

from fastapi import FastAPI

from app.models import (
    EnrichRequest,
    ExtractRequest,
    ExtractResponse,
    FetchRequest,
    FetchResponse,
    SearchRequest,
    SearchResponse,
    SearchResultItem,
)
from app.providers import arxiv, github, kaggle, openalex, scrapling_search, semantic_scholar, stealth, zenodo

app = FastAPI(title="Modeler Sidecar", version="0.1.0")


# Heuristic: if StealthyFetcher returned 0 items AND took >20s, it's almost
# certainly a timeout (STEALTHY_KWARGS.timeout is 50s). Surface as a warning
# so the Rust caller can fast-fall-back instead of waiting the full 60s
# reqwest timeout before its own retry logic kicks in.
_SCRAPLING_SLOW_THRESHOLD_SECS = 20.0


def _wrap_scrapling_search(
    raw_items: list[dict],
    t0: float,
    source_name: str,
) -> SearchResponse:
    """Convert raw scrapling dicts to a SearchResponse and detect likely timeouts.

    A scrapling call that returns 0 items in <2s almost certainly means the
    HTML structure changed (selector rot). A call that returns 0 items in
    20-50s almost certainly hit StealthyFetcher's timeout. We can't tell from
    the Python side without inspecting the exception, so we use the
    elapsed-time heuristic to flag the slow case for the caller.
    """
    items = [SearchResultItem(**item) for item in raw_items]
    warning: str | None = None
    if not items:
        elapsed = time.monotonic() - t0
        if elapsed >= _SCRAPLING_SLOW_THRESHOLD_SECS:
            warning = (
                f"Scrapling {source_name} returned 0 items after {elapsed:.1f}s "
                f"(StealthyFetcher timeout likely — caller should fast-fallback)"
            )
            logging.getLogger(__name__).info(warning)
    return SearchResponse(items=items, warning=warning)


@app.get("/health")
async def health():
    return {"status": "ok"}


@app.post("/search/papers", response_model=SearchResponse)
async def search_papers(req: SearchRequest):
    providers = [
        ("arxiv", arxiv.search),
        ("semantic_scholar", semantic_scholar.search),
        ("openalex", openalex.search),
    ]
    return await _aggregate_search(providers, req.query, req.limit)


@app.post("/search/papers/scrapling/arxiv", response_model=SearchResponse)
async def search_papers_scrapling_arxiv(req: SearchRequest):
    """arxiv.org HTML search via Scrapling (no API key needed).

    PRIMARY path for paper search — the existing /search/papers (which uses
    the REST-API arxiv library) is the FALLBACK when this returns 0 results.
    """
    raw = await scrapling_search.search_arxiv_html(req.query, req.limit)
    items = [SearchResultItem(**item) for item in raw]
    return SearchResponse(items=items, warning=None)


@app.post("/search/papers/scrapling/pubmed", response_model=SearchResponse)
async def search_papers_scrapling_pubmed(req: SearchRequest):
    """pubmed.ncbi.nlm.nih.gov HTML search via Scrapling.

    PRIMARY path for biomedical paper search. PubMed has no anti-bot, so this
    uses the basic Fetcher (no Chromium needed) — much faster than
    StealthyFetcher-based paths.
    """
    from app.providers.scrapling_search import search_pubmed_html
    raw = await search_pubmed_html(req.query, req.limit)
    items = [SearchResultItem(**item) for item in raw]
    return SearchResponse(items=items, warning=None)


@app.post("/search/papers/scrapling/semanticscholar", response_model=SearchResponse)
async def search_papers_scrapling_s2(req: SearchRequest):
    """semanticscholar.org HTML search via Scrapling StealthyFetcher.

    Uses Scrapling's StealthyFetcher (Chromium + stealth + Cloudflare bypass)
    since S2 is heavily anti-bot protected. Returns empty list on any failure
    so the Rust side can fall back to the API-based S2 search.
    """
    from app.providers.scrapling_search import search_semanticscholar_html
    t0 = time.monotonic()
    raw = await search_semanticscholar_html(req.query, req.limit)
    return _wrap_scrapling_search(raw, t0, "semanticscholar")


@app.post("/search/datasets", response_model=SearchResponse)
async def search_datasets(req: SearchRequest):
    providers = [
        ("zenodo", zenodo.search),
        ("kaggle", kaggle.search),
        ("github", github.search),
    ]
    return await _aggregate_search(providers, req.query, req.limit)


@app.post("/search/datasets/scrapling/zenodo", response_model=SearchResponse)
async def search_datasets_scrapling_zenodo(req: SearchRequest):
    """Zenodo dataset search via the InvenioRDM REST API.

    PRIMARY path for dataset search. Zenodo migrated to an InvenioRDM
    SPA, so the legacy HTML scrape (`scrapling_search.search_zenodo_html`,
    selectors `div.result-item` / `article.result`) returns 0 because the
    search results are JS-rendered after page load. The canonical
    InvenioRDM API at `/api/records` returns the same data as JSON and
    works without a browser — used here directly.

    The endpoint name keeps the `/scrapling/` prefix for backward compat
    with the Rust caller (`research.rs::search_zenodo_html_scrapling`).
    """
    items = await zenodo.search(req.query, req.limit)
    return SearchResponse(items=items, warning=None)


@app.post("/search/datasets/scrapling/kaggle", response_model=SearchResponse)
async def search_datasets_scrapling_kaggle(req: SearchRequest):
    """kaggle.com HTML search via Scrapling StealthyFetcher.

    PRIMARY path for Kaggle datasets — Kaggle renders search results via JS
    and has anti-bot, so the basic Fetcher cannot scrape them. Without
    Chromium installed this returns 0 items; callers should fall back to the
    Kaggle REST API in that case.
    """
    from app.providers.scrapling_search import search_kaggle_html
    t0 = time.monotonic()
    raw = await search_kaggle_html(req.query, req.limit)
    return _wrap_scrapling_search(raw, t0, "kaggle")


@app.post("/search/code", response_model=SearchResponse)
async def search_code(req: SearchRequest):
    providers = [
        ("github", github.search),
    ]
    return await _aggregate_search(providers, req.query, req.limit)


@app.post("/search/code/scrapling/github", response_model=SearchResponse)
async def search_code_scrapling_github(req: SearchRequest):
    """github.com HTML search via Scrapling StealthyFetcher.

    PRIMARY path for code search — the existing /search/code (which uses the
    REST GitHub API) is the FALLBACK when this returns 0 results. Note:
    GitHub requires login for >10 results; without auth, expect 5-10 repos.
    """
    t0 = time.monotonic()
    raw = await scrapling_search.search_github_html(req.query, req.limit)
    return _wrap_scrapling_search(raw, t0, "github")


@app.post("/enrich", response_model=SearchResultItem | None)
async def enrich(req: EnrichRequest):
    return await stealth.enrich_url(req.url)


@app.post("/search/web", response_model=SearchResponse)
async def search_web_endpoint(req: SearchRequest):
    """Web search via DDG HTML scraping (no API key needed)."""
    items = await stealth.search_web(req.query, req.limit)
    # search_web returns dicts; convert to SearchResultItem
    result_items = [SearchResultItem(**item) for item in items]
    return SearchResponse(items=result_items, warning=None)


@app.post("/fetch", response_model=FetchResponse)
async def fetch_endpoint(req: FetchRequest):
    """StealthyFetcher -> Selector -> text. Returns clean content for LLM consumption."""
    payload = await stealth.fetch_page(req.url, markdown=req.markdown, css=req.css)
    return FetchResponse(**payload)


@app.post("/extract", response_model=ExtractResponse)
async def extract_endpoint(req: ExtractRequest):
    """Selector + CSS selectors OR Fetcher.auto_match. Returns typed field dict."""
    payload = await stealth.extract(
        req.url,
        fields=req.fields,
        selector_hints=req.selector_hints,
    )
    return ExtractResponse(
        url=payload.get("url", req.url),
        fields=payload.get("fields", {}),
    )


async def _aggregate_search(providers, query: str, limit: int) -> SearchResponse:
    per_provider = max(2, (limit + len(providers) - 1) // len(providers))
    tasks = [fn(query, per_provider) for _, fn in providers]
    results = await asyncio.gather(*tasks, return_exceptions=True)

    items: list[SearchResultItem] = []
    warnings: list[str] = []

    for (name, _), result in zip(providers, results):
        if isinstance(result, Exception):
            warnings.append(f"{name}: {_short_error(result)}")
        else:
            items.extend(result)

    seen_urls: set[str] = set()
    deduped: list[SearchResultItem] = []
    for item in items:
        key = item.url.lower().rstrip("/")
        if key and key in seen_urls:
            continue
        seen_urls.add(key)
        deduped.append(item)

    deduped.sort(key=lambda x: x.relevance_score, reverse=True)

    return SearchResponse(
        items=deduped[:limit],
        warning="; ".join(warnings) if warnings else None,
    )


def _short_error(exc: Exception) -> str:
    msg = str(exc)
    if len(msg) > 120:
        return msg[:120] + "..."
    return msg or traceback.format_exception_only(type(exc), exc)[0].strip()
