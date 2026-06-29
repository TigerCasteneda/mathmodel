"""Scrapling-powered stealth extraction.

All Scrapling calls are wrapped in ``asyncio.to_thread`` so the FastAPI event loop
never blocks. Selector/Adaptor instances are constructed per-request (not shared).
"""
from __future__ import annotations

import asyncio
from typing import Any
from urllib.parse import parse_qs, quote_plus, urlparse

from app.models import SearchResultItem

# --- Constants -------------------------------------------------------------

# Anti-bot bypass via Scrapling's StealthyFetcher (Chromium + stealth).
# `solve_cloudflare=True` enables automatic Cloudflare challenge handling.
#
# Valid kwargs for Scrapling 0.4.9 StealthSession (per TypedDict in
# scrapling/engines/_browsers/_types.py): headless, solve_cloudflare,
# allow_webgl, hide_canvas, block_webrtc, plus all PlaywrightSession kwargs
# (timeout, proxy, etc.). NO `geoip` — including it causes a 500 error from
# Playwright when it tries to apply the unknown option to the browser
# context (verified against Scrapling 0.4.9 at D:/SASU/AI/mathmodel/Scrapling/).
STEALTHY_KWARGS: dict[str, Any] = dict(
    headless=True,
    solve_cloudflare=True,
    # 50s leaves a ~10s buffer under the Rust-side 60s HTTP timeout so the
    # sidecar can return a clean JSON response (with a warning) instead of
    # having the reqwest client cut the connection mid-stream. Chromium +
    # Cloudflare bypass can legitimately take 25-40s on a slow link.
    timeout=50_000,
)

# Plain Fetcher (curl_cffi) — used for trusted sites where speed > anti-bot.
FETCHER_KWARGS: dict[str, Any] = dict(
    timeout=30_000,
)


# --- Threaded primitives ---------------------------------------------------

async def _stealthy_get(url: str) -> Any:
    """Synchronous StealthyFetcher.fetch wrapped in asyncio.to_thread."""
    from scrapling import StealthyFetcher
    return await asyncio.to_thread(StealthyFetcher.fetch, url, **STEALTHY_KWARGS)


async def _fetcher_get(url: str) -> Any:
    """Non-stealthy Fetcher.get wrapped in asyncio.to_thread."""
    from scrapling import Fetcher
    return await asyncio.to_thread(Fetcher.get, url, **FETCHER_KWARGS)


# --- Public API ------------------------------------------------------------

async def fetch_page(url: str, *, markdown: bool = True, css: str | None = None) -> dict[str, Any]:
    """StealthyFetcher -> Selector. Returns a dict with title/text/links/images/status.

    Used by Rust's ``tool_fetch_url`` so the LLM gets a clean text payload instead
    of stripped HTML. ``css`` optionally narrows extraction to a sub-element.
    """
    from scrapling import Selector

    page = await _stealthy_get(url)
    # `page` is a `Response` (which extends `Selector`), but we re-wrap in a
    # fresh Selector so the caller can re-use it without sharing state.
    selector = Selector(content=page.html_content, url=getattr(page, "url", url))

    target = selector.css(css).first if css else selector.css("main, article, body").first
    target = target or selector

    # Title: prefer <title> text, else URL.
    title_node = page.css("title::text").get()
    title = (title_node or url).strip()

    # Text content (markdown mode uses Scrapling's get_all_text for clean output).
    if markdown:
        text_payload: str | None = target.get_all_text(strip=True) if hasattr(target, "get_all_text") else str(target.text)
    else:
        text_payload = None

    return {
        "title": title,
        "markdown": text_payload,
        "links": [
            r.attrib.get("href", "") if hasattr(r, "attrib") else r.attrib["href"]
            for r in page.css("a[href]")[:50]
            if (r.attrib.get("href") if hasattr(r, "attrib") else r.attrib.get("href"))
        ],
        "images": [
            r.attrib.get("src", "") if hasattr(r, "attrib") else r.attrib["src"]
            for r in page.css("img[src]")[:20]
            if (r.attrib.get("src") if hasattr(r, "attrib") else r.attrib.get("src"))
        ],
        "status": getattr(page, "status", 200),
    }


async def extract(
    url: str,
    *,
    fields: list[str] | None = None,
    selector_hints: dict[str, str] | None = None,
) -> dict[str, Any]:
    """Selector-based extraction. Two modes:

    - ``selector_hints`` given: CSS/XPath per field, returns node text or attr.
    - else ``fields`` given: try common heuristics (id/class match) for each
      field, falling back to the page's text content for the model to parse.
      This is the path used by the LLM when it has no prior knowledge of the
      page structure.

    Note: ``Fetcher.auto_match`` is not available in Scrapling 0.4.x, so we
    cannot delegate to AI-driven schema matching here. The LLM should pass
    ``selector_hints`` for known sites (arXiv abstract, GitHub repo, etc.) and
    use ``fetch_url`` for unknown pages.
    """
    from scrapling import Selector

    page = await _stealthy_get(url)
    selector = Selector(content=page.html_content, url=getattr(page, "url", url))
    result: dict[str, Any] = {}

    if selector_hints:
        for name, sel in selector_hints.items():
            try:
                node = selector.css(sel).first
                if node is None:
                    result[name] = None
                else:
                    # Prefer attribute values when selector hints at one (::attr(href) etc).
                    # Otherwise fall back to text content.
                    if hasattr(node, "attrib"):
                        attrib = node.attrib
                        result[name] = (
                            attrib.get("href")
                            or attrib.get("content")
                            or (str(node.text) if node.text else None)
                        )
                    else:
                        result[name] = str(node)
            except Exception:
                result[name] = None
    elif fields:
        # Heuristic mode: try id/class/tag matches for each field name, then
        # fall back to the page's text content under ``_text`` so the model can
        # parse the result itself. Field names are lowercased for matching.
        for name in fields:
            try:
                key = name.lower().replace(" ", "-")
                node = (
                    selector.css(f"#{key}").first
                    or selector.css(f".{key}").first
                    or selector.css(f"meta[name='{key}']").first
                )
                if node is not None:
                    if hasattr(node, "attrib") and (node.attrib.get("content") or node.attrib.get("href")):
                        result[name] = node.attrib.get("content") or node.attrib.get("href")
                    elif hasattr(node, "text") and node.text:
                        result[name] = str(node.text).strip()
                    else:
                        result[name] = None
                else:
                    result[name] = None
            except Exception:
                result[name] = None
        # Always include the page text so the model can extract from raw markdown
        # when heuristics fail.
        try:
            text_node = selector.css("main, article, body").first
            result["_text"] = str(text_node.get_all_text(strip=True)) if text_node else ""
        except Exception:
            result["_text"] = ""

    return {"url": url, "fields": result}


async def search_web(query: str, limit: int = 8) -> list[dict[str, Any]]:
    """Scrape DDG HTML, extract result links via Selector.

    Returns dicts compatible with ``SearchResultItem``. On any failure returns
    an empty list so the caller can fall back gracefully.
    """
    from scrapling import Selector

    url = f"https://html.duckduckgo.com/html/?q={quote_plus(query)}"
    try:
        page = await _stealthy_get(url)
    except Exception:
        return []

    selector = Selector(content=page.html_content, url=getattr(page, "url", url))
    # Real Scrapling has no extract_many — iterate per-result and use .css() to
    # pull title/url/snippet from each <div class="result"> container.
    result_nodes = selector.css("div.result")

    items: list[dict[str, Any]] = []
    for i, node in enumerate(result_nodes[:limit]):
        try:
            title = node.css("h2 a::text").get()
            raw_url = node.css("h2 a::attr(href)").get()
            snippet = node.css(".result__snippet::text").get()
        except Exception:
            continue

        norm = _normalise_ddg_url(raw_url or "")
        items.append({
            "title": (title or "").strip(),
            "url": norm,
            "content": (snippet or "").strip(),
            "provider": "scrapling",
            "category": "web",
            "relevance_score": max(0.1, 1.0 - i * 0.05),
            "raw_json": {"ddg_raw": raw_url},
        })
    return items


def _normalise_ddg_url(href: str) -> str:
    """DDG wraps external links as ``//duckduckgo.com/l/?uddg=<encoded>``."""
    if not href:
        return ""
    if href.startswith("//"):
        href = "https:" + href
    if not href.startswith("http"):
        return href
    try:
        parsed = urlparse(href)
        qs = parse_qs(parsed.query)
        uddg = qs.get("uddg", [None])[0]
        if uddg:
            return uddg
    except Exception:
        pass
    return href


# --- Backward-compatible enrich_url ----------------------------------------

async def enrich_url(url: str) -> SearchResultItem | None:
    """Fetch a URL and return a SearchResultItem for backward compat with /enrich."""
    try:
        page = await fetch_page(url, markdown=True)
    except Exception:
        return None

    if not page:
        return None

    return SearchResultItem(
        title=page.get("title") or url,
        url=url,
        content=(page.get("markdown") or "")[:3000],
        provider="stealth",
        category="literature",
        relevance_score=1.0,
        raw_json={"enriched": True, "scrapling": True},
    )
