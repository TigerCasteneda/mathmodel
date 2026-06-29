"""Scrapling-based HTML search scrapers.

Each function in this module scrapes a real academic source's HTML search
results page and returns a list of dicts compatible with `SearchResultItem`.
This is the PRIMARY search path for the agentic researcher — the original
REST-API sidecar (`/search/papers` etc.) is the FALLBACK.

Scraping policy:
- arxiv.org, pubmed.ncbi.nlm.nih.gov, zenodo.org have no anti-bot — use Fetcher.
- semanticscholar.org, github.com, kaggle.com have anti-bot — use StealthyFetcher.
- All Scrapling calls are wrapped in `asyncio.to_thread` to keep the FastAPI
  event loop responsive.
- On any failure (timeout, anti-bot block, parse error) return an empty list
  so the caller can fall back to the API path.
"""
from __future__ import annotations

import asyncio
import logging
import time
from typing import Any
from urllib.parse import quote_plus

# Reuse the proven primitives from stealth.py so we don't duplicate threading logic.
from app.providers.stealth import _fetcher_get, _stealthy_get, STEALTHY_KWARGS


__all__ = [
    "search_arxiv_html",
    "search_semanticscholar_html",
    "search_pubmed_html",
    "search_github_html",
    "search_kaggle_html",
]


# Each function is added below by its dedicated implementation agent.
# They all follow the same signature:
#   async def search_<source>_html(query: str, limit: int) -> list[dict[str, Any]]
#   returns: list of dicts compatible with SearchResultItem


async def search_pubmed_html(query: str, limit: int = 8) -> list[dict[str, Any]]:
    """Scrape pubmed.ncbi.nlm.nih.gov's search results page.

    PubMed has no anti-bot — uses the basic Fetcher. The search page is at
    `https://pubmed.ncbi.nlm.nih.gov/?term={q}&sort=relevance`. Each result
    is a `<div class="docsum-content">` block with:
      - a.docsum-title   → paper title + URL (PMID-based)
      - span.docsum-authors   → author list (truncated with "...")
      - div.docsum-snippet    → abstract snippet
      - span.docsum-journal-citation  → journal + year
    """
    import re

    from scrapling import Selector

    url = f"https://pubmed.ncbi.nlm.nih.gov/?term={quote_plus(query)}&sort=relevance"
    try:
        page = await _fetcher_get(url)
    except Exception:
        return []

    selector = Selector(content=page.html_content, url=getattr(page, "url", url))
    nodes = (
        selector.css("div.docsum-content")
        or selector.css("article.full-docsum")
    )
    if hasattr(nodes, "__iter__") and not isinstance(nodes, str):
        node_list = list(nodes)
    else:
        node_list = nodes if nodes is not None else []

    items: list[dict[str, Any]] = []
    for i, node in enumerate(node_list[:limit]):
        try:
            title_node = node.css("a.docsum-title").first
            if title_node is None:
                continue
            # PubMed highlights query terms with nested <b> tags inside
            # a.docsum-title and div.docsum-snippet. Scrapling's `.text` only
            # returns the direct text node, so use `get_all_text()` to walk
            # descendants and recover the full highlighted title/snippet.
            title_raw = (
                title_node.get_all_text(strip=True)
                if hasattr(title_node, "get_all_text")
                else (title_node.text or "")
            )
            title = " ".join(str(title_raw).split())
            url_final = (title_node.attrib.get("href") or "").strip()
            if url_final.startswith("/"):
                url_final = f"https://pubmed.ncbi.nlm.nih.gov{url_final}"

            authors_node = node.css("span.docsum-authors").first
            authors_text = ""
            if authors_node is not None and authors_node.text:
                authors_text = " ".join(authors_node.text.split())
                # Strip trailing "..." marker
                if authors_text.endswith("..."):
                    authors_text = authors_text[:-3].strip() + " et al."

            snippet_node = node.css("div.docsum-snippet").first
            snippet = ""
            if snippet_node is not None:
                snippet_raw = (
                    snippet_node.get_all_text(strip=True)
                    if hasattr(snippet_node, "get_all_text")
                    else (snippet_node.text or "")
                )
                snippet = " ".join(str(snippet_raw).split())

            year = None
            journal_node = node.css("span.docsum-journal-citation").first
            if journal_node is not None and journal_node.text:
                m = re.search(r"\b(19|20)\d{2}\b", journal_node.text)
                if m:
                    year = int(m.group(0))

            if not url_final or not title:
                continue
            if "pubmed.ncbi.nlm.nih.gov" not in url_final:
                continue

            items.append({
                "title": title,
                "url": url_final,
                "content": snippet or title,
                "provider": "scrapling_pubmed",
                "source": "scrapling_pubmed_search",
                "category": "literature",
                "authors": authors_text or None,
                "publish_year": year,
                "keywords": None,
                "relevance_score": max(0.1, 1.0 - i * 0.05),
                "raw_json": {},
            })
        except Exception:
            continue
    return items


async def search_arxiv_html(query: str, limit: int = 8) -> list[dict[str, Any]]:
    """Scrape arxiv.org's search results page and extract paper blocks.

    arxiv.org has no anti-bot — uses the basic Fetcher. The search page is
    at ``https://arxiv.org/search/?query={q}&searchtype=all``. Each result is
    an ``<li class="arxiv-result">`` block with:

    - ``p.list-title > a``   → paper title + URL
    - ``p.authors > a``      → author names
    - ``p.abstract``         → abstract text (truncated by arxiv with "...")
    - ``p.submission-history`` → submission date with year
    - ``p.arxiv-id``         → arxiv ID (e.g. arXiv:2401.00001)

    Returns dicts compatible with ``SearchResultItem``. On any failure returns
    an empty list so the caller can fall back gracefully.
    """
    import re
    from scrapling import Selector

    url = f"https://arxiv.org/search/?query={quote_plus(query)}&searchtype=all&start=0"
    try:
        page = await _fetcher_get(url)
    except Exception:
        return []

    selector = Selector(content=page.html_content, url=getattr(page, "url", url))
    nodes = selector.css("li.arxiv-result")
    if hasattr(nodes, "__iter__") and not isinstance(nodes, str):
        node_list = list(nodes)
    else:
        node_list = nodes if nodes is not None else []

    items: list[dict[str, Any]] = []
    for i, node in enumerate(node_list[:limit]):
        try:
            # Real arxiv.org structure: the paper title lives in `p.title` and
            # the link in `p.list-title a` points to the abstract page (e.g.
            # https://arxiv.org/abs/1507.01661). Older/legacy scrapes may also
            # have used `p.list-title a` as the title, so we accept either.
            title = ""
            url_final = ""
            title_node = node.css("p.title").first
            if title_node is not None and title_node.text:
                title = " ".join(title_node.text.split()).strip()
            list_title_link = node.css("p.list-title a").first
            if list_title_link is not None:
                href = list_title_link.attrib.get("href", "") or ""
                if href and not href.startswith("http"):
                    href = f"https://arxiv.org{href}"
                url_final = href.strip()
            if not title and list_title_link is not None:
                # Legacy / fallback: title is the link text
                title = (list_title_link.text or "").strip()

            authors_node = node.css("p.authors")
            authors_text = ""
            if authors_node is not None:
                # `authors_node` may be a `Selectors` collection; iterate it
                # and pull the <a> children for author names.
                author_links = authors_node.css("a")
                if author_links:
                    names = [a.text.strip() for a in author_links if a.text]
                    authors_text = ", ".join(names)

            abstract_node = node.css("p.abstract")
            abstract = ""
            if abstract_node is not None:
                # Real Scrapling returns a `Selectors` collection for `.css`
                # calls; concatenate the text of all matched abstract blocks.
                try:
                    abstract_text = abstract_node.text or ""
                except AttributeError:
                    abstract_text = ""
                if abstract_text:
                    abstract = " ".join(abstract_text.split())
                    abstract = abstract.replace("Abstract:", "").strip()
                    if abstract.endswith("…"):
                        abstract = abstract[:-1].strip() + " ..."

            year: int | None = None
            # The submission history is rendered in a small text under
            # `p.submission-history` (legacy) or as a "Submitted <date>" line
            # inside the result metadata (current). Look in both.
            history_text = ""
            history_node = node.css("p.submission-history")
            if history_node is not None:
                try:
                    history_text = history_node.text or ""
                except AttributeError:
                    history_text = ""
            if not history_text:
                # Fallback: scan the whole block text for a year.
                try:
                    history_text = node.text or ""
                except AttributeError:
                    history_text = ""

            if history_text:
                m = re.search(r"\b(19|20)\d{2}\b", history_text)
                if m:
                    year = int(m.group(0))

            arxiv_id = ""
            # The arxiv id is the last URL path segment.
            if url_final:
                m = re.search(r"/(abs|pdf)/([0-9]{4}\.[0-9]{4,5})", url_final)
                if m:
                    arxiv_id = f"arXiv:{m.group(2)}"

            if not url_final or not title:
                continue

            items.append({
                "title": title,
                "url": url_final,
                "content": abstract or title,
                "provider": "scrapling_arxiv",
                "source": "scrapling_arxiv_search",
                "category": "literature",
                "authors": authors_text or None,
                "publish_year": year,
                "keywords": None,
                "relevance_score": max(0.1, 1.0 - i * 0.05),
                "raw_json": {"arxiv_id": arxiv_id} if arxiv_id else {},
            })
        except Exception:
            continue
    return items


async def search_semanticscholar_html(query: str, limit: int = 8) -> list[dict[str, Any]]:
    """Scrape semanticscholar.org's search results page.

    Semantic Scholar has Cloudflare protection — use StealthyFetcher with
    anti-bot bypass. The page may render via JavaScript; if so, we may need
    DynamicFetcher instead. Try StealthyFetcher first; if it returns 0
    results, we silently fall through (caller will try a different scraper).
    """
    import re

    from scrapling import Selector

    url = f"https://www.semanticscholar.org/search?q={quote_plus(query)}&sort=relevance"
    _t0 = time.monotonic()
    try:
        page = await _stealthy_get(url)
    except Exception as exc:
        # Don't silently swallow — selector rot, StealthyFetcher timeout,
        # and network failures all land here; the caller needs to know
        # which it was. Elapsed time tells us if it was a timeout
        # (close to STEALTHY_KWARGS.timeout) vs an instant parse failure.
        logging.getLogger(__name__).warning(
            "Scrapling S2 HTML fetch failed for query=%r after %.1fs: %s: %s",
            query,
            time.monotonic() - _t0,
            type(exc).__name__,
            exc,
        )
        return []

    selector = Selector(content=page.html_content, url=getattr(page, "url", url))

    # Try multiple selector strategies — S2's HTML structure has changed over
    # time, and we want resilience to layout changes.
    nodes = (
        selector.css("article[data-testid='search-result']")
        or selector.css("div.cl-paper-row")
        or selector.css("div.result-page__paper")
        or selector.css("li.result-item")
    )
    if hasattr(nodes, "__iter__") and not isinstance(nodes, str):
        node_list = list(nodes)
    else:
        node_list = nodes if nodes is not None else []

    items: list[dict[str, Any]] = []
    for i, node in enumerate(node_list[:limit]):
        try:
            # Title + URL — try multiple selectors
            title_node = (
                node.css("h3 a").first
                or node.css("a[data-testid='title-link']").first
                or node.css(".cl-paper-title a").first
            )
            if title_node is None:
                continue
            title = (title_node.text or "").strip()
            url_final = (title_node.attrib.get("href") or "").strip()
            if url_final.startswith("/"):
                url_final = f"https://www.semanticscholar.org{url_final}"

            # Authors
            authors_node = (
                node.css("[data-testid='authors']").first
                or node.css(".cl-paper-authors").first
                or node.css(".authors").first
            )
            authors_text = ""
            if authors_node is not None:
                author_links = authors_node.css("a")
                if author_links:
                    names = [a.text.strip() for a in author_links if a.text]
                    authors_text = ", ".join(names)
                elif authors_node.text:
                    authors_text = " ".join(authors_node.text.split())

            # Snippet / abstract
            snippet_node = (
                node.css("[data-testid='snippet']").first
                or node.css(".cl-paper-snippet").first
                or node.css("p").first
            )
            snippet = ""
            if snippet_node is not None and snippet_node.text:
                snippet = " ".join(snippet_node.text.split())

            # Year
            year = None
            for year_sel in [
                "[data-testid='year']",
                ".cl-paper-year",
                ".year",
            ]:
                year_node = node.css(year_sel).first
                if year_node is not None and year_node.text:
                    m = re.search(r"\b(19|20)\d{2}\b", year_node.text)
                    if m:
                        year = int(m.group(0))
                        break

            if not url_final or not title:
                continue
            if "semanticscholar.org" not in url_final:
                continue  # only accept S2 paper links

            items.append({
                "title": title,
                "url": url_final,
                "content": snippet or title,
                "provider": "scrapling_s2",
                "source": "scrapling_s2_search",
                "category": "literature",
                "authors": authors_text or None,
                "publish_year": year,
                "keywords": None,
                "relevance_score": max(0.1, 1.0 - i * 0.05),
                "raw_json": {},
            })
        except Exception:
            continue
    return items


async def search_github_html(query: str, limit: int = 8) -> list[dict[str, Any]]:
    """Scrape github.com's repository search results page.

    GitHub has anti-bot — uses StealthyFetcher. Note: GitHub requires login
    to see more than ~10 results; without auth, expect 5-10 repos per page.
    The caller should fall back to the GitHub API for deeper results.

    GitHub uses CSS-modules with hashed class names that change on every
    deploy (e.g. ``Result-module__Result__Up5vk``), so we rely on stable
    structural hooks:

      - Container row:   ``[data-testid='results-list'] > div``
      - Title anchor:    ``a[data-component='Link']`` whose href is exactly
                         ``/owner/repo`` (skip ``/stargazers``, ``/topics/...``,
                         ``/issues`` etc. that share the same component)
      - Title text:      derived from the href — more reliable than
                         ``title_node.text`` which Scrapling can strip of
                         nested ``<span>``/``<em>`` matches
      - Description:     ``[class*='Content-module__Content']`` (current)
                         with legacy ``p.color-fg-muted`` as fallback

    The old ``div.SearchResult`` / ``article.Box-row`` selectors currently
    return 0 (github migrated off them) but are kept as last-resort in case
    of rollback.
    """
    import logging
    from scrapling import Selector

    url = f"https://github.com/search?q={quote_plus(query)}&type=repositories"
    _t0 = time.monotonic()
    try:
        page = await _stealthy_get(url)
    except Exception as exc:
        # Don't silently swallow — selector rot, StealthyFetcher timeout,
        # and network failures all land here; the caller needs to know
        # which it was. Elapsed time tells us if it was a timeout
        # (close to STEALTHY_KWARGS.timeout) vs an instant parse failure.
        logging.getLogger(__name__).warning(
            "Scrapling github HTML fetch failed for query=%r after %.1fs: %s: %s",
            query,
            time.monotonic() - _t0,
            type(exc).__name__,
            exc,
        )
        return []

    selector = Selector(content=page.html_content, url=getattr(page, "url", url))
    # Stable hook first; legacy classes as last-resort fallback.
    nodes = (
        selector.css("[data-testid='results-list'] > div")
        or selector.css("div.SearchResult")
        or selector.css("article.Box-row")
    )

    # Selector.css may return a single Adaptor or a list of Adaptors depending
    # on the match type. Normalize to a list for uniform iteration.
    if hasattr(nodes, "__iter__") and not isinstance(nodes, str):
        try:
            node_list = list(nodes)
        except TypeError:
            node_list = [nodes] if nodes is not None else []
    else:
        node_list = [nodes] if nodes is not None else []

    items: list[dict[str, Any]] = []
    for i, node in enumerate(node_list[:limit]):
        try:
            # Prefer the new ``a[data-component='Link']`` with a clean
            # ``/owner/repo`` href. Skip footer links (/stargazers has 3
            # segments, /topics/foo is to a topic page, /issues/... etc.).
            link_select = node.css("a[data-component='Link']")
            if hasattr(link_select, "__iter__") and not isinstance(link_select, str):
                anchor_candidates = list(link_select)
            else:
                first = link_select.first if hasattr(link_select, "first") else link_select
                anchor_candidates = [first] if first is not None else []

            title_node = None
            for anchor in anchor_candidates:
                href = (anchor.attrib.get("href") or "").strip()
                parts = [p for p in href.lstrip("/").split("/") if p]
                if len(parts) == 2 and all("." not in p for p in parts):
                    title_node = anchor
                    break
            if title_node is None:
                # Legacy fallbacks
                title_node = (
                    node.css("h3 a").first
                    or node.css("a.v-align-middle").first
                    or node.css("a[data-testid='result-repo-link']").first
                )
            if title_node is None:
                continue

            # Derive title from href (more reliable than .text which can be
            # stripped of nested <span>/<em> children by Scrapling).
            url_final = (title_node.attrib.get("href") or "").strip()
            if url_final.startswith("/"):
                url_final = f"https://github.com{url_final}"
            owner_repo = url_final.replace("https://github.com/", "").strip("/")
            title = " / ".join(p for p in owner_repo.split("/") if p)
            if not title:
                # Last resort: scrape visible text.
                raw_title = (title_node.text or "").strip()
                title = " / ".join(
                    [
                        p.strip()
                        for p in raw_title.replace("\n", " ").split("/")
                        if p.strip()
                    ]
                )

            desc_node = (
                node.css("[class*='Content-module__Content']").first
                or node.css("p.color-fg-muted").first
                or node.css("div.SearchResult-second-line").first
            )
            desc = ""
            if desc_node is not None:
                # Scrapling's ``.text`` strips nested <span>/<em> children
                # (it returns direct text nodes only), which empties the
                # description on the current GitHub layout. Use
                # ``get_all_text(strip=True)`` to recurse into descendants.
                raw_desc = ""
                if hasattr(desc_node, "get_all_text"):
                    raw_desc = desc_node.get_all_text(strip=True) or ""
                if not raw_desc:
                    raw_desc = (desc_node.text or "")
                desc = " ".join(raw_desc.split())

            if not url_final or not title:
                continue
            if "github.com" not in url_final:
                continue
            # Filter to only repo URLs (skip user/org profile pages)
            parts = url_final.replace("https://github.com/", "").split("/")
            if len(parts) < 2:
                continue

            items.append({
                "title": title,
                "url": url_final,
                "content": desc or title,
                "provider": "scrapling_github",
                "source": "scrapling_github_search",
                "category": "code",
                "authors": parts[0] if parts else None,
                "publish_year": None,  # GitHub doesn't show year in search results
                "keywords": None,
                "relevance_score": max(0.1, 1.0 - i * 0.05),
                "raw_json": {},
            })
        except Exception:
            continue
    return items


async def search_kaggle_html(query: str, limit: int = 8) -> list[dict[str, Any]]:
    """Scrape kaggle.com's search results page.

    Kaggle has anti-bot AND renders search results via JavaScript — uses
    StealthyFetcher (headless Chromium with stealth). Without Chromium
    installed, this will return 0 items; the caller should fall back to
    the Kaggle API.

    The search page is at ``https://www.kaggle.com/search?q={q}``. Result
    blocks are typically ``<a href="/datasets/owner/name">`` cards.

    Returns dicts compatible with ``SearchResultItem``. On any failure returns
    an empty list so the caller can fall back gracefully.
    """
    from scrapling import Selector

    url = f"https://www.kaggle.com/search?q={quote_plus(query)}"
    _t0 = time.monotonic()
    try:
        page = await _stealthy_get(url)
    except Exception as exc:
        # Don't silently swallow — selector rot, StealthyFetcher timeout,
        # and network failures all land here; the caller needs to know
        # which it was. Elapsed time tells us if it was a timeout
        # (close to STEALTHY_KWARGS.timeout) vs an instant parse failure.
        logging.getLogger(__name__).warning(
            "Scrapling kaggle HTML fetch failed for query=%r after %.1fs: %s: %s",
            query,
            time.monotonic() - _t0,
            type(exc).__name__,
            exc,
        )
        return []

    selector = Selector(content=page.html_content, url=getattr(page, "url", url))
    # Kaggle's structure: each dataset card is wrapped in an <a href="/datasets/...">.
    nodes = (
        selector.css("a[href*='/datasets/']")
        or selector.css("div.dataset-item")
        or selector.css("article.sc-fzqMdJ")
    )
    # Selector.css may return a single Adaptor or a list of Adaptors depending
    # on the match type. Normalize to a list for uniform iteration.
    if hasattr(nodes, "__iter__") and not isinstance(nodes, str):
        try:
            node_list = list(nodes)
        except TypeError:
            node_list = [nodes] if nodes is not None else []
    else:
        node_list = [nodes] if nodes is not None else []

    items: list[dict[str, Any]] = []
    seen_urls: set[str] = set()
    for i, node in enumerate(node_list):
        if len(items) >= limit:
            break
        try:
            url_final = (node.attrib.get("href") or "").strip() if hasattr(node, "attrib") else ""
            if url_final.startswith("/"):
                url_final = f"https://www.kaggle.com{url_final}"
            if not url_final or "kaggle.com" not in url_final:
                continue
            # Normalize: /datasets/owner/name or /datasets/owner/name?param=...
            url_final = url_final.split("?")[0].rstrip("/")
            if not url_final.startswith("https://www.kaggle.com/datasets/"):
                continue
            if url_final in seen_urls:
                continue
            seen_urls.add(url_final)

            # Derive path parts once — used for the URL-derived title and authors.
            path_parts = url_final.replace("https://www.kaggle.com/datasets/", "").split("/")

            # Title: from the card text first.
            title = (node.text or "").strip() if hasattr(node, "text") and node.text else ""
            if not title:
                # Try a child h2/h3/h4.
                t = node.css("h2, h3, h4").first
                if t is not None and t.text:
                    title = t.text.strip()
            if not title:
                # Derive from URL: /datasets/owner/dataset-name → "dataset-name by owner".
                if len(path_parts) >= 2:
                    title = f"{path_parts[-1]} by {path_parts[0]}"
                else:
                    continue

            # Description: try a child <p>.
            desc = ""
            desc_node = node.css("p").first
            if desc_node is not None and desc_node.text:
                desc = " ".join(desc_node.text.split())

            items.append({
                "title": title,
                "url": url_final,
                "content": desc or title,
                "provider": "scrapling_kaggle",
                "source": "scrapling_kaggle_search",
                "category": "dataset",
                "authors": path_parts[0] if len(path_parts) >= 2 else None,
                "publish_year": None,
                "keywords": None,
                "relevance_score": max(0.1, 1.0 - i * 0.05),
                "raw_json": {},
            })
        except Exception:
            continue
    return items
