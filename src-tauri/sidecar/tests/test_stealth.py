"""Tests for stealth.py — uses monkeypatch to avoid real network calls.

The tests inject a fake `scrapling` module into sys.modules so the implementation
can import StealthyFetcher/Fetcher/Selector without a real install.
"""
import sys
from unittest.mock import MagicMock

import pytest


class FakePage:
    """Mimics Scrapling's Response/Selector just enough for our usage."""

    def __init__(self, html: str = "<html></html>", status: int = 200, url: str = ""):
        self.html_content = html
        self.text = html
        self.status = status
        self.url = url

    def css(self, selector: str):
        # Adaptor-style: .css().first, .css().get(), .css().extract_many()
        chain = MagicMock()
        chain.first = None
        chain.get.return_value = ""
        chain.extract_many.return_value = []
        return chain


@pytest.fixture
def fake_scrapling(monkeypatch):
    """Patch scrapling classes before each test.

    Provides:
      - StealthyFetcher.fetch(url) -> page
      - Fetcher.get(url) -> page
      - Selector(content, url) -> selector-like object with .css(), .text, .attrib
    """
    fake_module = MagicMock()
    fake_stealthy_cls = MagicMock()
    fake_fetcher_cls = MagicMock()
    fake_selector_cls = MagicMock()

    # Default Selector instance behaviour: empty results
    fake_selector_instance = MagicMock()
    fake_selector_instance.css.return_value.first = None
    fake_selector_instance.css.return_value.extract_many.return_value = []
    fake_selector_instance.to_markdown.return_value = "# Title\n\nMarkdown content"
    fake_selector_cls.return_value = fake_selector_instance

    fake_module.StealthyFetcher = fake_stealthy_cls
    fake_module.Fetcher = fake_fetcher_cls
    fake_module.Selector = fake_selector_cls

    monkeypatch.setitem(sys.modules, "scrapling", fake_module)
    return {
        "stealthy_cls": fake_stealthy_cls,
        "fetcher_cls": fake_fetcher_cls,
        "selector_cls": fake_selector_cls,
        "selector_instance": fake_selector_instance,
    }


@pytest.mark.asyncio
async def test_fetch_page_returns_expected_shape(fake_scrapling):
    """fetch_page should return {title, markdown, links, images, status}."""
    from app.providers import stealth

    fake_page = FakePage("<html><title>Hello</title><body>body</body></html>")
    fake_scrapling["stealthy_cls"].fetch.return_value = fake_page
    # selector.css("main, article, body").first falls back to page
    fake_scrapling["selector_instance"].css.return_value.first = fake_page
    fake_scrapling["selector_instance"].css.return_value.get.return_value = "Hello"
    # title extraction uses page.css("title::text").get()
    # The chain.first is None so to_markdown on page won't be called; we use fallback.

    result = await stealth.fetch_page("https://example.com")

    assert "title" in result
    assert "markdown" in result
    assert "links" in result
    assert "images" in result
    assert "status" in result
    assert result["status"] == 200


@pytest.mark.asyncio
async def test_extract_uses_selector_hints(fake_scrapling):
    """When selector_hints is provided, extract uses them and does NOT call auto_match."""
    from app.providers import stealth

    fake_page = FakePage("<html><h1>Test H1</h1></html>")
    fake_scrapling["stealthy_cls"].fetch.return_value = fake_page

    fake_node = MagicMock()
    fake_node.text = "Test H1"
    fake_node.attrib = {}
    fake_scrapling["selector_instance"].css.return_value.first = fake_node

    result = await stealth.extract(
        "https://example.com",
        selector_hints={"heading": "h1::text"},
    )
    assert result["fields"].get("heading") == "Test H1"
    # auto_match on Fetcher should NOT be invoked when hints are provided
    assert not fake_scrapling["fetcher_cls"].auto_match.called


@pytest.mark.asyncio
async def test_extract_uses_heuristic_field_match(fake_scrapling):
    """When only fields is provided (no hints), extract uses id/class heuristics
    and falls back to including the page text under ``_text`` for the model to parse.

    Note: ``Fetcher.auto_match`` does NOT exist in Scrapling 0.4.x — we test the
    real heuristic path instead.
    """
    from app.providers import stealth

    fake_page = FakePage("<html><body><span id='author'>Alice</span></body></html>")
    fake_scrapling["stealthy_cls"].fetch.return_value = fake_page

    # First css() call inside extract(): `selector.css("#author")` — return a node
    # Second css() call: `selector.css("main, article, body").first` — for the _text fallback
    author_node = MagicMock()
    author_node.attrib = {}
    author_node.text = "Alice"

    body_node = MagicMock()
    body_node.get_all_text.return_value = "body content here"

    def css_side_effect(sel):
        chain = MagicMock()
        if sel == "#author":
            chain.first = author_node
        elif "main" in sel or "body" in sel:
            chain.first = body_node
        else:
            chain.first = None
        return chain

    fake_scrapling["selector_instance"].css.side_effect = css_side_effect

    result = await stealth.extract(
        "https://example.com",
        fields=["author"],
    )
    # The author field should be filled by the id match
    assert result["fields"].get("author") == "Alice"
    # The _text fallback should be present so the model can parse other fields
    assert "_text" in result["fields"]


@pytest.mark.asyncio
async def test_search_web_normalises_ddg_url(fake_scrapling):
    """DDG redirect URLs should be unwrapped to the real target."""
    from app.providers import stealth

    fake_page = FakePage("<html></html>")
    fake_scrapling["stealthy_cls"].fetch.return_value = fake_page

    # search_web iterates `selector.css("div.result")`; make it return one
    # node whose sub-css() returns the DDG redirect href.
    fake_node = MagicMock()
    fake_node.css.side_effect = lambda sel: MagicMock(
        get=MagicMock(return_value="//duckduckgo.com/l/?uddg=https%3A%2F%2Farxiv.org%2Fabs%2F2401.00001"
        if "href" in sel else ("abstract" if "snippet" in sel else "Test"))
    )
    # selector.css("div.result") returns a list of these fake nodes
    fake_scrapling["selector_instance"].css.return_value = [fake_node]

    items = await stealth.search_web("test query", limit=5)
    assert len(items) == 1
    assert items[0]["url"] == "https://arxiv.org/abs/2401.00001"
    assert items[0]["provider"] == "scrapling"
    assert items[0]["category"] == "web"


@pytest.mark.asyncio
async def test_search_web_returns_empty_on_failure(fake_scrapling):
    """DDG fetch failure returns empty list, not propagated exception."""
    from app.providers import stealth

    fake_scrapling["stealthy_cls"].fetch.side_effect = Exception("timeout")

    items = await stealth.search_web("test query", limit=5)
    assert items == []


def test_normalise_ddg_url_with_uddg():
    from app.providers import stealth

    result = stealth._normalise_ddg_url(
        "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpath"
    )
    assert result == "https://example.com/path"


def test_normalise_ddg_url_passthrough():
    from app.providers import stealth

    assert stealth._normalise_ddg_url("https://example.com") == "https://example.com"
    assert stealth._normalise_ddg_url("") == ""


@pytest.mark.asyncio
async def test_enrich_url_keeps_signature(fake_scrapling):
    """enrich_url must accept (url) and return SearchResultItem-shaped object or None."""
    from app.providers import stealth

    fake_page = FakePage("<html><title>Hi</title><body>body</body></html>")
    fake_scrapling["stealthy_cls"].fetch.return_value = fake_page
    fake_scrapling["selector_instance"].css.return_value.first = fake_page
    fake_scrapling["selector_instance"].css.return_value.get.return_value = "Hi"

    result = await stealth.enrich_url("https://example.com")
    assert result is not None
    assert result.url == "https://example.com"
    assert result.provider == "stealth"


@pytest.mark.asyncio
async def test_enrich_url_returns_none_on_failure(fake_scrapling):
    from app.providers import stealth

    fake_scrapling["stealthy_cls"].fetch.side_effect = Exception("boom")
    result = await stealth.enrich_url("https://example.com")
    assert result is None


def test_stealthy_kwargs_include_cloudflare_bypass():
    """STEALTHY_KWARGS must request anti-bot bypass."""
    from app.providers import stealth

    assert stealth.STEALTHY_KWARGS.get("solve_cloudflare") is True
    assert stealth.STEALTHY_KWARGS.get("headless") is True
