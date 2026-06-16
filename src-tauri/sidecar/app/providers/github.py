import os

import httpx

from app.models import SearchResultItem

BASE_URL = "https://api.github.com/search/repositories"


async def search(query: str, limit: int) -> list[SearchResultItem]:
    params = {
        "q": query,
        "sort": "stars",
        "order": "desc",
        "per_page": min(limit, 50),
    }
    headers = {
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "ModelerAI/1.0",
    }
    # Optional token lifts the rate limit from 10 to 30 requests/min. Inherited
    # from the environment the sidecar was launched in; unauthenticated works too.
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"

    async with httpx.AsyncClient(timeout=20, follow_redirects=True) as client:
        resp = await client.get(BASE_URL, params=params, headers=headers)
        resp.raise_for_status()

    data = resp.json()
    repos = data.get("items") or []
    items: list[SearchResultItem] = []

    for repo in repos[:limit]:
        full_name = repo.get("full_name") or repo.get("name") or "Untitled"
        url = repo.get("html_url") or ""
        description = repo.get("description") or ""
        owner = (repo.get("owner") or {}).get("login")
        stars = repo.get("stargazers_count") or 0
        language = repo.get("language") or ""
        topics = repo.get("topics") or []

        year = None
        pushed = repo.get("pushed_at") or repo.get("updated_at") or ""
        if len(pushed) >= 4:
            try:
                year = int(pushed[:4])
            except ValueError:
                pass

        keyword_parts = [p for p in ([language] + topics) if p]

        items.append(SearchResultItem(
            title=full_name,
            url=url,
            content=description,
            provider="github",
            category="code",
            authors=owner,
            publish_year=year,
            keywords=", ".join(keyword_parts[:8]) if keyword_parts else None,
            relevance_score=min(1.0, stars / 1000) if stars else 0.3,
            raw_json={"stars": stars, "language": language, "full_name": full_name},
        ))

    return items
