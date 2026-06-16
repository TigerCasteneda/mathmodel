import asyncio
import traceback

from fastapi import FastAPI

from app.models import EnrichRequest, SearchRequest, SearchResponse, SearchResultItem
from app.providers import arxiv, github, kaggle, openalex, semantic_scholar, stealth, zenodo

app = FastAPI(title="Modeler Sidecar", version="0.1.0")


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


@app.post("/search/datasets", response_model=SearchResponse)
async def search_datasets(req: SearchRequest):
    providers = [
        ("zenodo", zenodo.search),
        ("kaggle", kaggle.search),
        ("github", github.search),
    ]
    return await _aggregate_search(providers, req.query, req.limit)


@app.post("/search/code", response_model=SearchResponse)
async def search_code(req: SearchRequest):
    providers = [
        ("github", github.search),
    ]
    return await _aggregate_search(providers, req.query, req.limit)


@app.post("/enrich", response_model=SearchResultItem | None)
async def enrich(req: EnrichRequest):
    return await stealth.enrich_url(req.url)


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
