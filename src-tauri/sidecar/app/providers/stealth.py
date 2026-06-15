import httpx

from app.models import SearchResultItem


async def enrich_url(url: str) -> SearchResultItem | None:
    """Fetch a URL and extract basic metadata. Uses Scrapling if available, else plain httpx."""
    try:
        page_html = await _fetch_html(url)
    except Exception:
        return None

    if not page_html:
        return None

    title = _extract_title(page_html)
    content = _extract_text_snippet(page_html, max_len=3000)

    return SearchResultItem(
        title=title or url,
        url=url,
        content=content,
        provider="stealth",
        category="literature",
        relevance_score=1.0,
        raw_json={"enriched": True},
    )


async def _fetch_html(url: str) -> str:
    try:
        from scrapling import Fetcher
        page = Fetcher.get(url, timeout=15000)
        return page.html_content if hasattr(page, "html_content") else str(page)
    except ImportError:
        pass

    async with httpx.AsyncClient(
        timeout=15,
        follow_redirects=True,
        headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"},
    ) as client:
        resp = await client.get(url)
        resp.raise_for_status()
        return resp.text


def _extract_title(html: str) -> str | None:
    lower = html.lower()
    start = lower.find("<title")
    if start == -1:
        return None
    close = lower.find(">", start)
    if close == -1:
        return None
    end = lower.find("</title>", close)
    if end == -1:
        return None
    return html[close + 1:end].strip() or None


def _extract_text_snippet(html: str, max_len: int = 3000) -> str:
    in_tag = False
    chars: list[str] = []
    for ch in html:
        if ch == "<":
            in_tag = True
        elif ch == ">":
            in_tag = False
            chars.append(" ")
        elif not in_tag:
            chars.append(ch)
        if len(chars) >= max_len * 2:
            break
    text = "".join(chars)
    return " ".join(text.split())[:max_len]
