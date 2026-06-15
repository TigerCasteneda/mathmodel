import xml.etree.ElementTree as ET

import httpx

from app.models import SearchResultItem

ARXIV_API = "https://export.arxiv.org/api/query"
NS = {
    "atom": "http://www.w3.org/2005/Atom",
    "arxiv": "http://arxiv.org/schemas/atom",
}


async def search(query: str, limit: int) -> list[SearchResultItem]:
    params = {
        "search_query": f"all:{query}",
        "start": 0,
        "max_results": limit,
        "sortBy": "relevance",
        "sortOrder": "descending",
    }
    async with httpx.AsyncClient(timeout=30, follow_redirects=True) as client:
        resp = await client.get(ARXIV_API, params=params)
        resp.raise_for_status()

    root = ET.fromstring(resp.text)
    items: list[SearchResultItem] = []

    for entry in root.findall("atom:entry", NS):
        title = _text(entry, "atom:title").replace("\n", " ").strip()
        abstract = _text(entry, "atom:summary").strip()
        arxiv_id = _text(entry, "atom:id")
        url = arxiv_id if arxiv_id else ""

        authors = ", ".join(
            name.text.strip()
            for author in entry.findall("atom:author", NS)
            if (name := author.find("atom:name", NS)) is not None and name.text
        )

        published = _text(entry, "atom:published")
        year = int(published[:4]) if len(published) >= 4 else None

        categories = [
            cat.get("term", "")
            for cat in entry.findall("atom:category", NS)
        ]

        items.append(SearchResultItem(
            title=title,
            url=url,
            content=abstract,
            provider="arxiv",
            category="literature",
            authors=authors or None,
            publish_year=year,
            keywords=", ".join(categories[:5]) if categories else None,
            relevance_score=1.0 - (len(items) * 0.05),
            raw_json={"arxiv_id": arxiv_id, "categories": categories},
        ))

    return items


def _text(element: ET.Element, tag: str) -> str:
    node = element.find(tag, NS)
    return (node.text or "").strip() if node is not None else ""
