import httpx

from app.models import SearchResultItem

BASE_URL = "https://zenodo.org/api/records"


async def search(query: str, limit: int) -> list[SearchResultItem]:
    params = {
        "q": query,
        "size": min(limit, 50),
        "type": "dataset",
        "sort": "mostrecent",
    }
    async with httpx.AsyncClient(timeout=20) as client:
        resp = await client.get(BASE_URL, params=params)
        resp.raise_for_status()

    data = resp.json()
    hits = data.get("hits", {}).get("hits", [])
    items: list[SearchResultItem] = []

    for record in hits[:limit]:
        metadata = record.get("metadata", {})
        title = metadata.get("title") or "Untitled"
        doi = record.get("doi") or ""
        record_id = record.get("id", "")
        url = f"https://doi.org/{doi}" if doi else f"https://zenodo.org/records/{record_id}"

        description = metadata.get("description") or ""
        description = _strip_html(description)[:2000]

        creators = metadata.get("creators") or []
        authors = ", ".join(
            c.get("name", "") for c in creators[:10]
        )

        year = None
        pub_date = metadata.get("publication_date") or ""
        if len(pub_date) >= 4:
            try:
                year = int(pub_date[:4])
            except ValueError:
                pass

        keywords_list = metadata.get("keywords") or []

        items.append(SearchResultItem(
            title=title,
            url=url,
            content=description,
            provider="zenodo",
            category="dataset",
            authors=authors or None,
            publish_year=year,
            keywords=", ".join(keywords_list[:8]) if keywords_list else None,
            relevance_score=0.7,
            raw_json={"zenodo_id": record_id, "doi": doi},
        ))

    return items


def _strip_html(text: str) -> str:
    import re
    return re.sub(r"<[^>]+>", " ", text).strip()
