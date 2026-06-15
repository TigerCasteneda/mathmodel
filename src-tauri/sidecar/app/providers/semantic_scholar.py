import httpx

from app.models import SearchResultItem

BASE_URL = "https://api.semanticscholar.org/graph/v1/paper/search"
FIELDS = "title,url,abstract,authors,year,citationCount,fieldsOfStudy"


async def search(query: str, limit: int) -> list[SearchResultItem]:
    params = {
        "query": query,
        "limit": min(limit, 100),
        "fields": FIELDS,
    }
    async with httpx.AsyncClient(timeout=30) as client:
        resp = await client.get(BASE_URL, params=params)
        if resp.status_code == 429:
            return []
        resp.raise_for_status()

    data = resp.json()
    papers = data.get("data") or []
    items: list[SearchResultItem] = []

    for paper in papers[:limit]:
        title = paper.get("title") or "Untitled"
        url = paper.get("url") or ""
        abstract = paper.get("abstract") or ""
        authors_list = paper.get("authors") or []
        authors = ", ".join(a.get("name", "") for a in authors_list[:10])
        year = paper.get("year")
        fields = paper.get("fieldsOfStudy") or []
        citations = paper.get("citationCount") or 0

        items.append(SearchResultItem(
            title=title,
            url=url,
            content=abstract,
            provider="semantic_scholar",
            category="literature",
            authors=authors or None,
            publish_year=year,
            keywords=", ".join(fields[:5]) if fields else None,
            relevance_score=min(1.0, citations / 100) if citations else 0.5,
            raw_json={"paper_id": paper.get("paperId"), "citations": citations},
        ))

    return items
