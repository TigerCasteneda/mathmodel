import httpx

from app.models import SearchResultItem

BASE_URL = "https://api.openalex.org/works"


async def search(query: str, limit: int) -> list[SearchResultItem]:
    params = {
        "search": query,
        "per_page": min(limit, 50),
        "select": "id,doi,title,display_name,publication_year,authorships,abstract_inverted_index,cited_by_count,concepts,primary_location",
    }
    headers = {"User-Agent": "ModelerAI/1.0 (mailto:research@example.com)"}

    async with httpx.AsyncClient(timeout=30, follow_redirects=True) as client:
        resp = await client.get(BASE_URL, params=params, headers=headers)
        resp.raise_for_status()

    data = resp.json()
    works = data.get("results") or []
    items: list[SearchResultItem] = []

    for work in works[:limit]:
        title = work.get("display_name") or work.get("title") or "Untitled"
        doi = work.get("doi") or ""
        url = doi if doi else work.get("id", "")

        location = work.get("primary_location") or {}
        source = location.get("source") or {}
        if not url and source.get("landing_page_url"):
            url = source["landing_page_url"]

        abstract = _reconstruct_abstract(work.get("abstract_inverted_index"))
        year = work.get("publication_year")
        citations = work.get("cited_by_count") or 0

        authorships = work.get("authorships") or []
        authors = ", ".join(
            a["author"]["display_name"]
            for a in authorships[:10]
            if a.get("author", {}).get("display_name")
        )

        concepts = work.get("concepts") or []
        keywords = ", ".join(
            c["display_name"] for c in concepts[:5] if c.get("display_name")
        )

        items.append(SearchResultItem(
            title=title,
            url=url,
            content=abstract,
            provider="openalex",
            category="literature",
            authors=authors or None,
            publish_year=year,
            keywords=keywords or None,
            relevance_score=min(1.0, citations / 100) if citations else 0.3,
            raw_json={"openalex_id": work.get("id"), "citations": citations},
        ))

    return items


def _reconstruct_abstract(inverted_index: dict | None) -> str:
    if not inverted_index:
        return ""
    positions: list[tuple[int, str]] = []
    for word, indices in inverted_index.items():
        for idx in indices:
            positions.append((idx, word))
    positions.sort()
    return " ".join(word for _, word in positions)
