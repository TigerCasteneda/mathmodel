import httpx

from app.models import SearchResultItem

KAGGLE_SEARCH_URL = "https://www.kaggle.com/api/i/datasets.DatasetService/SearchDatasets"


async def search(query: str, limit: int) -> list[SearchResultItem]:
    payload = {
        "query": query,
        "limit": min(limit, 20),
    }
    headers = {
        "Content-Type": "application/json",
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
    }

    try:
        async with httpx.AsyncClient(timeout=20) as client:
            resp = await client.post(KAGGLE_SEARCH_URL, json=payload, headers=headers)
            if resp.status_code != 200:
                return await _fallback_search(query, limit)
            data = resp.json()
    except Exception:
        return await _fallback_search(query, limit)

    datasets = data.get("datasetResults") or data.get("datasets") or []
    items: list[SearchResultItem] = []

    for ds in datasets[:limit]:
        title = ds.get("title") or ds.get("name") or "Untitled"
        slug = ds.get("datasetSlug") or ds.get("ref") or ""
        owner = ds.get("ownerName") or ds.get("ownerRef") or ""
        url = f"https://www.kaggle.com/datasets/{owner}/{slug}" if owner and slug else ""
        subtitle = ds.get("subtitle") or ds.get("description") or ""
        size = ds.get("totalBytes") or 0
        downloads = ds.get("downloadCount") or ds.get("totalDownloads") or 0

        items.append(SearchResultItem(
            title=title,
            url=url,
            content=subtitle,
            provider="kaggle",
            category="dataset",
            authors=owner or None,
            keywords=None,
            relevance_score=min(1.0, downloads / 1000) if downloads else 0.4,
            raw_json={"size_bytes": size, "downloads": downloads},
        ))

    return items


async def _fallback_search(query: str, limit: int) -> list[SearchResultItem]:
    """Fallback: use Kaggle public datasets API (requires no auth for listing)."""
    params = {"search": query, "page": 1, "pageSize": min(limit, 20)}
    headers = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
    }
    try:
        async with httpx.AsyncClient(timeout=15) as client:
            resp = await client.get(
                "https://www.kaggle.com/api/v1/datasets/list",
                params=params,
                headers=headers,
            )
            if resp.status_code != 200:
                return []
            datasets = resp.json()
    except Exception:
        return []

    items: list[SearchResultItem] = []
    for ds in datasets[:limit]:
        ref = ds.get("ref") or ""
        title = ds.get("title") or ref
        url = f"https://www.kaggle.com/datasets/{ref}" if ref else ""
        subtitle = ds.get("subtitle") or ""

        items.append(SearchResultItem(
            title=title,
            url=url,
            content=subtitle,
            provider="kaggle",
            category="dataset",
            relevance_score=0.4,
            raw_json=ds,
        ))

    return items
