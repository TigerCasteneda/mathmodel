"""Tests for scrapling_search.py — uses the same fake_scrapling pattern as test_stealth.py.

We import `fake_scrapling` indirectly: scrapling_search.py uses the same
`_fetcher_get` / `_stealthy_get` helpers from `app.providers.stealth`, so we
can build a parallel local fixture here that injects the same fake module.
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

    def css(self, selector: str):  # pragma: no cover - unused in this file
        chain = MagicMock()
        chain.first = None
        chain.get.return_value = ""
        chain.extract_many.return_value = []
        return chain


@pytest.fixture
def fake_scrapling(monkeypatch):
    """Patch scrapling classes before each test (same as test_stealth)."""
    fake_module = MagicMock()
    fake_stealthy_cls = MagicMock()
    fake_fetcher_cls = MagicMock()
    fake_selector_cls = MagicMock()

    fake_selector_instance = MagicMock()
    fake_selector_instance.css.return_value.first = None
    fake_selector_instance.css.return_value.extract_many.return_value = []
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
async def test_search_arxiv_html_returns_expected_shape(fake_scrapling):
    """search_arxiv_html should return arxiv-shaped items.

    Uses the MagicMock-based fixture: this only verifies that the function
    does not raise on the basic call path with a real-shape page. The
    lxml-backed tests below pin the actual extraction semantics.
    """
    from app.providers.scrapling_search import search_arxiv_html

    html = """
    <html><body>
    <li class="arxiv-result">
        <p class="list-title"><a href="https://arxiv.org/abs/2401.00001">Test Paper Title</a></p>
        <p class="authors"><a>Alice</a>, <a>Bob</a></p>
        <p class="abstract">Abstract: This is a test paper about something</p>
        <p class="submission-history">Submitted 1 January 2024; v1 submitted 1 January 2024</p>
        <p class="arxiv-id"><a>arXiv:2401.00001</a></p>
    </li>
    </body></html>
    """
    fake_page = FakePage(html)
    fake_scrapling["fetcher_cls"].get.return_value = fake_page

    items = await search_arxiv_html("test query", limit=5)
    # With MagicMock-based selector, field extraction returns None values so
    # the function short-circuits. We only assert that the call does not
    # raise. Real extraction is covered by the lxml-backed tests below.
    assert isinstance(items, list)


@pytest.mark.asyncio
async def test_search_arxiv_html_returns_empty_on_failure(fake_scrapling):
    """Network failure should return empty list, not raise."""
    from app.providers.scrapling_search import search_arxiv_html

    fake_scrapling["fetcher_cls"].get.side_effect = Exception("timeout")
    items = await search_arxiv_html("test", limit=5)
    assert items == []


@pytest.mark.asyncio
async def test_search_arxiv_html_skips_blocks_without_url(fake_scrapling):
    """Blocks missing a URL or title are skipped, not erroring the whole result.

    The MagicMock-based selector cannot validate that the function *finds*
    items; that is covered by the lxml-backed test. Here we just assert the
    function does not raise on a typical two-block input.
    """
    from app.providers.scrapling_search import search_arxiv_html

    html = """
    <html><body>
    <li class="arxiv-result">
        <p class="list-title"><a>No URL Here</a></p>
    </li>
    <li class="arxiv-result">
        <p class="list-title"><a href="https://arxiv.org/abs/2401.00002">Real Paper</a></p>
    </li>
    </body></html>
    """
    fake_page = FakePage(html)
    fake_scrapling["fetcher_cls"].get.return_value = fake_page

    items = await search_arxiv_html("test", limit=5)
    assert isinstance(items, list)


# --- arxiv scraper (lxml-backed) -----------------------------------------


class FakeArxivNode:
    """Per-node stub mimicking Scrapling's element wrapper.

    Carries a backing lxml element so sub-selectors like `p.list-title a`
    actually filter descendants, mirroring Scrapling's real behaviour.
    """

    def __init__(self, lxml_element=None, text="", attrib=None, children=None):
        # Backing lxml element (used for cssselect on sub-queries).
        self._lxml = lxml_element
        self.text = text
        self.attrib = attrib or {}
        self._children = children or []

    def css(self, selector):
        if self._lxml is not None:
            try:
                matched = self._lxml.cssselect(selector)
                nodes = [
                    FakeArxivNode(
                        lxml_element=m,
                        text=(m.text or "").strip(),
                        attrib={"href": m.get("href", "")} if m.tag == "a" else {},
                    )
                    for m in matched
                ]
                return FakeArxivCssChain(nodes)
            except Exception:
                pass
        return FakeArxivCssChain(self._children)


class FakeArxivCssChain:
    """Chain with `.first`, `.text`, `.css(...)`, and iterable semantics matching Scrapling."""

    def __init__(self, nodes=None):
        self._nodes = nodes or []
        self.first = self._nodes[0] if self._nodes else None

    @property
    def text(self):
        # Concatenate text from all backing lxml elements so production code
        # that does `node.css("p.abstract").text` works the way it does on
        # the real Scrapling chain.
        chunks: list[str] = []
        for node in self._nodes:
            lxml_el = getattr(node, "_lxml", None)
            if lxml_el is not None:
                try:
                    chunks.append(lxml_el.text_content() or "")
                except Exception:
                    pass
            elif node.text:
                chunks.append(node.text)
        return " ".join(chunks).strip()

    def css(self, selector):
        # Apply the selector against each node's backing lxml element and
        # collect the matches. This is what allows ``chain.css("a")`` on a
        # chain returned by ``node.css("p.authors")`` to work.
        nodes: list[FakeArxivNode] = []
        for node in self._nodes:
            lxml_el = getattr(node, "_lxml", None)
            if lxml_el is None:
                continue
            try:
                for m in lxml_el.cssselect(selector):
                    nodes.append(
                        FakeArxivNode(
                            lxml_element=m,
                            text=(m.text or "").strip(),
                            attrib={"href": m.get("href", "")} if m.tag == "a" else {},
                        )
                    )
            except Exception:
                continue
        return FakeArxivCssChain(nodes)

    def get(self, *_args, **_kwargs):  # pragma: no cover - unused here
        return ""

    def extract_many(self):  # pragma: no cover - unused here
        return []

    def __iter__(self):
        return iter(self._nodes)

    def __bool__(self):
        return bool(self._nodes)


def _parse_arxiv_html(html):
    """Return {selector: [FakeArxivNode, ...]} for arxiv-result blocks in *html*.

    The wrapper `li.arxiv-result` node carries a backing lxml element so that
    chained `.css(...)` calls on the node (e.g. ``p.list-title a``) correctly
    filter descendants.
    """
    from lxml import html as lxml_html

    tree = lxml_html.fromstring(html)
    blocks = tree.cssselect("li.arxiv-result")
    by_selector: dict[str, list[FakeArxivNode]] = {
        "li.arxiv-result": [FakeArxivNode(lxml_element=block) for block in blocks]
    }
    for block in blocks:
        # title link: p.list-title > a
        title_links = block.cssselect("p.list-title a")
        if title_links:
            t = title_links[0]
            by_selector.setdefault("p.list-title a", []).append(
                FakeArxivNode(
                    lxml_element=t,
                    text=(t.text or "").strip(),
                    attrib={"href": t.get("href", "")},
                )
            )
        # p.authors -> with its <a> children as sub-nodes
        authors_ps = block.cssselect("p.authors")
        if authors_ps:
            ap = authors_ps[0]
            by_selector.setdefault("p.authors", []).append(
                FakeArxivNode(lxml_element=ap, text=ap.text_content() or "")
            )
        # p.abstract
        abstract_ps = block.cssselect("p.abstract")
        if abstract_ps:
            ap = abstract_ps[0]
            by_selector.setdefault("p.abstract", []).append(
                FakeArxivNode(lxml_element=ap, text=ap.text_content() or "")
            )
        # p.submission-history
        history_ps = block.cssselect("p.submission-history")
        if history_ps:
            hp = history_ps[0]
            by_selector.setdefault("p.submission-history", []).append(
                FakeArxivNode(lxml_element=hp, text=hp.text_content() or "")
            )
        # p.arxiv-id a
        arxiv_id_links = block.cssselect("p.arxiv-id a")
        if arxiv_id_links:
            t = arxiv_id_links[0]
            by_selector.setdefault("p.arxiv-id a", []).append(
                FakeArxivNode(
                    lxml_element=t,
                    text=(t.text or "").strip(),
                    attrib={"href": t.get("href", "")},
                )
            )
    return by_selector


def _build_arxiv_fake_scrapling(monkeypatch, html):
    """Inject a Selector stub that parses the provided HTML as arxiv-result blocks."""
    block_nodes_by_selector = _parse_arxiv_html(html)
    fake_module = MagicMock()
    fake_stealthy_cls = MagicMock()
    fake_fetcher_cls = MagicMock()
    fake_selector_cls = MagicMock()

    def _selector_factory(content=None, url=None):
        instance = MagicMock()

        def _css(selector):
            nodes = block_nodes_by_selector.get(selector, [])
            return FakeArxivCssChain(nodes)

        instance.css.side_effect = _css
        return instance

    fake_selector_cls.side_effect = _selector_factory
    fake_module.StealthyFetcher = fake_stealthy_cls
    fake_module.Fetcher = fake_fetcher_cls
    fake_module.Selector = fake_selector_cls
    monkeypatch.setitem(sys.modules, "scrapling", fake_module)
    return fake_fetcher_cls


@pytest.mark.asyncio
async def test_search_arxiv_html_lxml_returns_expected_shape(monkeypatch):
    """Lxml-backed selector should expose title/authors/year/arxiv-id correctly."""
    from app.providers.scrapling_search import search_arxiv_html

    html = """
    <html><body>
    <li class="arxiv-result">
        <p class="list-title"><a href="https://arxiv.org/abs/2401.00001">Test Paper Title</a></p>
        <p class="authors"><a>Alice</a>, <a>Bob</a></p>
        <p class="abstract">Abstract: This is a test paper about something</p>
        <p class="submission-history">Submitted 1 January 2024; v1 submitted 1 January 2024</p>
        <p class="arxiv-id"><a>arXiv:2401.00001</a></p>
    </li>
    </body></html>
    """
    fetcher_cls = _build_arxiv_fake_scrapling(monkeypatch, html)
    fetcher_cls.get.return_value = FakePage(html)

    items = await search_arxiv_html("test query", limit=5)
    assert len(items) == 1
    item = items[0]
    assert item["title"] == "Test Paper Title"
    assert item["url"] == "https://arxiv.org/abs/2401.00001"
    assert item["provider"] == "scrapling_arxiv"
    assert item["source"] == "scrapling_arxiv_search"
    assert item["category"] == "literature"
    assert item["publish_year"] == 2024
    assert "Alice" in item["authors"]
    assert "Bob" in item["authors"]
    assert item["raw_json"].get("arxiv_id") == "arXiv:2401.00001"


@pytest.mark.asyncio
async def test_search_arxiv_html_lxml_skips_blocks_without_url(monkeypatch):
    """Blocks missing a URL or title are skipped, not erroring the whole result."""
    from app.providers.scrapling_search import search_arxiv_html

    html = """
    <html><body>
    <li class="arxiv-result">
        <p class="list-title"><a>No URL Here</a></p>
    </li>
    <li class="arxiv-result">
        <p class="list-title"><a href="https://arxiv.org/abs/2401.00002">Real Paper</a></p>
    </li>
    </body></html>
    """
    fetcher_cls = _build_arxiv_fake_scrapling(monkeypatch, html)
    fetcher_cls.get.return_value = FakePage(html)

    items = await search_arxiv_html("test", limit=5)
    assert len(items) == 1
    assert items[0]["url"] == "https://arxiv.org/abs/2401.00002"


# --- PubMed scraper -------------------------------------------------------


class FakePubmedNode:
    """Per-node stub mimicking Scrapling's first-element pattern."""

    def __init__(self, text="", attrib=None):
        self.text = text
        self.attrib = attrib or {}


class FakePubmedCssChain:
    """Reusable chain supporting `.first` and `.extract_many()`."""

    def __init__(self, nodes=None):
        self._nodes = nodes or []
        self.first = self._nodes[0] if self._nodes else None

    def get(self, *_args, **_kwargs):  # pragma: no cover - unused here
        return ""

    def extract_many(self):  # pragma: no cover - unused here
        return []

    def __iter__(self):
        return iter(self._nodes)

    def __bool__(self):
        return bool(self._nodes)


def _build_pubmed_fake_scrapling(monkeypatch, html):
    """Inject a Selector stub that parses the provided HTML as docsum blocks.

    Each block returned by `selector.css("div.docsum-content")` is a FakePubmedNode
    whose `.css(sub_selector)` returns only the sub-nodes belonging to that
    specific block — mirroring how Scrapling's Adaptor.css works.
    """
    block_nodes_by_selector = _parse_pubmed_html(html)
    fake_module = MagicMock()
    fake_stealthy_cls = MagicMock()
    fake_fetcher_cls = MagicMock()
    fake_selector_cls = MagicMock()

    def _selector_factory(content=None, url=None):
        instance = MagicMock()

        def _css(selector):
            nodes = block_nodes_by_selector.get(selector, [])
            return FakePubmedCssChain(nodes)

        instance.css.side_effect = _css
        return instance

    fake_selector_cls.side_effect = _selector_factory
    fake_module.StealthyFetcher = fake_stealthy_cls
    fake_module.Fetcher = fake_fetcher_cls
    fake_module.Selector = fake_selector_cls
    monkeypatch.setitem(sys.modules, "scrapling", fake_module)
    return fake_fetcher_cls


def _parse_pubmed_html(html):
    """Return {selector: [FakePubmedNode, ...]} for the docsum blocks in *html*.

    Top-level selectors ("div.docsum-content", "article.full-docsum") return the
    block wrappers themselves; nested selectors ("a.docsum-title" etc.) return
    FakePubmedNode instances scoped per-block via the wrapper's `.css(sub)`.
    """
    from lxml import html as lxml_html

    tree = lxml_html.fromstring(html)
    blocks = tree.cssselect("div.docsum-content")
    if not blocks:
        blocks = tree.cssselect("article.full-docsum")

    wrappers: list[FakePubmedBlockWrapper] = []
    for block in blocks:
        sub_nodes: dict[str, list[FakePubmedNode]] = {}
        title_a = block.cssselect("a.docsum-title")
        if title_a:
            href = title_a[0].get("href", "")
            text = (title_a[0].text_content() or "").strip()
            sub_nodes.setdefault("a.docsum-title", []).append(
                FakePubmedNode(text=text, attrib={"href": href})
            )
        authors = block.cssselect("span.docsum-authors")
        if authors:
            sub_nodes.setdefault("span.docsum-authors", []).append(
                FakePubmedNode(text=authors[0].text_content() or "")
            )
        snippets = block.cssselect("div.docsum-snippet")
        if snippets:
            sub_nodes.setdefault("div.docsum-snippet", []).append(
                FakePubmedNode(text=snippets[0].text_content() or "")
            )
        journal = block.cssselect("span.docsum-journal-citation")
        if journal:
            sub_nodes.setdefault("span.docsum-journal-citation", []).append(
                FakePubmedNode(text=journal[0].text_content() or "")
            )
        wrappers.append(FakePubmedBlockWrapper(sub_nodes))

    by_selector = {
        "div.docsum-content": wrappers,
        "article.full-docsum": wrappers,
    }
    return by_selector


class FakePubmedBlockWrapper:
    """Scrapling-like Adaptor wrapper scoped to one docsum block."""

    def __init__(self, sub_nodes: dict[str, list[FakePubmedNode]]):
        self._sub_nodes = sub_nodes

    def css(self, sub_selector: str) -> FakePubmedCssChain:
        return FakePubmedCssChain(self._sub_nodes.get(sub_selector, []))


@pytest.mark.asyncio
async def test_search_pubmed_html_parses_docsum_blocks(monkeypatch):
    """Should extract title/url/authors/snippet from PubMed docsum blocks."""
    from app.providers.scrapling_search import search_pubmed_html

    html = """
    <html><body>
    <div class="docsum-content">
        <a class="docsum-title" href="/12345678/">Test Paper Title</a>
        <span class="docsum-authors">Smith J, Jones K, Brown L...</span>
        <div class="docsum-snippet">Background: This is a test paper about...</div>
        <span class="docsum-journal-citation">Nature. 2024 Jan;600(7890):123-130.</span>
    </div>
    </body></html>
    """
    fetcher_cls = _build_pubmed_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html)
    fetcher_cls.get.return_value = fake_page

    items = await search_pubmed_html("test", limit=5)
    assert len(items) == 1
    item = items[0]
    assert item["title"] == "Test Paper Title"
    assert item["url"] == "https://pubmed.ncbi.nlm.nih.gov/12345678/"
    assert item["provider"] == "scrapling_pubmed"
    assert item["source"] == "scrapling_pubmed_search"
    assert item["category"] == "literature"
    assert item["publish_year"] == 2024
    assert "Smith J" in item["authors"]
    assert "et al." in item["authors"]


@pytest.mark.asyncio
async def test_search_pubmed_html_returns_empty_on_failure(fake_scrapling):
    """Network failure should return empty list, not raise."""
    from app.providers.scrapling_search import search_pubmed_html

    fake_scrapling["fetcher_cls"].get.side_effect = Exception("timeout")
    items = await search_pubmed_html("test", limit=5)
    assert items == []


@pytest.mark.asyncio
async def test_search_pubmed_html_skips_blocks_without_url(monkeypatch):
    """Blocks missing a URL or title are skipped, not erroring the whole result."""
    from app.providers.scrapling_search import search_pubmed_html

    html = """
    <html><body>
    <div class="docsum-content">
        <span class="docsum-authors">NoTitleAuthor</span>
    </div>
    <div class="docsum-content">
        <a class="docsum-title" href="/87654321/">Real Paper</a>
        <span class="docsum-authors">Doe J</span>
        <span class="docsum-journal-citation">Cell. 2023 Mar.</span>
    </div>
    </body></html>
    """
    fetcher_cls = _build_pubmed_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html)
    fetcher_cls.get.return_value = fake_page

    items = await search_pubmed_html("test", limit=5)
    assert len(items) == 1
    assert items[0]["url"] == "https://pubmed.ncbi.nlm.nih.gov/87654321/"
    assert items[0]["publish_year"] == 2023


# --- Semantic Scholar scraper ---------------------------------------------


def _build_s2_fake_scrapling(monkeypatch, html):
    """Inject a Selector stub for S2 that uses lxml to parse real HTML.

    Follows the same pattern as the PubMed fixture — production-faithful
    parsing so the selector chain logic is genuinely exercised.
    """
    from lxml import html as lxml_html

    tree = lxml_html.fromstring(html)
    # Use any of the documented result-block selectors.
    blocks = (
        tree.cssselect("article[data-testid='search-result']")
        or tree.cssselect("div.cl-paper-row")
        or tree.cssselect("div.result-page__paper")
        or tree.cssselect("li.result-item")
    )

    class _Node:
        """Scrapling-like node: .text, .attrib, and .css() for inner lookups."""

        def __init__(self, text="", href="", children=None):
            self.text = text
            self.attrib = {"href": href} if href else {}
            self._children = children or {}

        def css(self, selector):
            return _Chain(self._children.get(selector, []))

    class _Chain:
        def __init__(self, nodes=None):
            self._nodes = nodes or []
            self.first = self._nodes[0] if self._nodes else None

        def __iter__(self):
            return iter(self._nodes)

        def __bool__(self):
            return bool(self._nodes)

        def __getitem__(self, i):
            return self._nodes[i]

        def __len__(self):
            return len(self._nodes)

    # Top-level selectors — implementation picks first non-empty via `or`.
    top_selectors = [
        "article[data-testid='search-result']",
        "div.cl-paper-row",
        "div.result-page__paper",
        "li.result-item",
    ]
    top_by_selector: dict[str, list[_Node]] = {s: [] for s in top_selectors}
    top_nodes: list[_Node] = []

    for block in blocks:
        # ---- title + URL --------------------------------------------------
        title_selectors = ["h3 a", "a[data-testid='title-link']", ".cl-paper-title a"]
        title_node = None
        for sel in title_selectors:
            try:
                hits = block.cssselect(sel)
            except Exception:
                hits = []
            if hits:
                title_node = _Node(
                    text=(hits[0].text_content() or "").strip(),
                    href=hits[0].get("href", ""),
                )
                break
        if title_node is None:
            continue  # block has no title → skip

        # ---- authors ------------------------------------------------------
        author_selectors = ["[data-testid='authors']", ".cl-paper-authors", ".authors"]
        author_inner: list[_Node] = []
        for sel in author_selectors:
            try:
                hits = block.cssselect(sel)
            except Exception:
                hits = []
            if hits:
                for a in hits[0].cssselect("a"):
                    name = (a.text_content() or "").strip()
                    if name:
                        author_inner.append(_Node(text=name))
                break
        authors_node = (
            _Node(text="", children={"a": author_inner})
            if author_inner
            else _Node(text="")
        )

        # ---- snippet ------------------------------------------------------
        snippet_selectors = ["[data-testid='snippet']", ".cl-paper-snippet", "p"]
        snippet_node = _Node(text="")
        for sel in snippet_selectors:
            try:
                hits = block.cssselect(sel)
            except Exception:
                hits = []
            if hits:
                snippet_node = _Node(text=(hits[0].text_content() or "").strip())
                break

        # ---- year ---------------------------------------------------------
        year_selectors = ["[data-testid='year']", ".cl-paper-year", ".year"]
        year_node = _Node(text="")
        for sel in year_selectors:
            try:
                hits = block.cssselect(sel)
            except Exception:
                hits = []
            if hits:
                year_node = _Node(text=(hits[0].text_content() or "").strip())
                break

        # Assemble per-block node with the inner-selector map the implementation
        # expects when calling node.css(...) on each result block.
        block_node = _Node(
            text="",
            children={
                "h3 a": [title_node],
                "a[data-testid='title-link']": [title_node],
                ".cl-paper-title a": [title_node],
                "[data-testid='authors']": [authors_node],
                ".cl-paper-authors": [authors_node],
                ".authors": [authors_node],
                "[data-testid='snippet']": [snippet_node],
                ".cl-paper-snippet": [snippet_node],
                "p": [snippet_node],
                "[data-testid='year']": [year_node],
                ".cl-paper-year": [year_node],
                ".year": [year_node],
            },
        )
        top_nodes.append(block_node)

    for s in top_selectors:
        top_by_selector[s] = list(top_nodes)

    fake_module = MagicMock()
    fake_stealthy_cls = MagicMock()
    fake_fetcher_cls = MagicMock()
    fake_selector_cls = MagicMock()

    def _selector_factory(content=None, url=None):
        instance = MagicMock()

        def _css(selector):
            return _Chain(top_by_selector.get(selector, []))

        instance.css.side_effect = _css
        return instance

    fake_selector_cls.side_effect = _selector_factory
    fake_module.StealthyFetcher = fake_stealthy_cls
    fake_module.Fetcher = fake_fetcher_cls
    fake_module.Selector = fake_selector_cls
    monkeypatch.setitem(sys.modules, "scrapling", fake_module)
    return fake_stealthy_cls


@pytest.mark.asyncio
async def test_search_semanticscholar_html_returns_expected_shape(monkeypatch):
    """Should extract title/url/authors/snippet/year from S2 result blocks."""
    from app.providers.scrapling_search import search_semanticscholar_html

    html = """
    <html><body>
    <article data-testid='search-result'>
        <h3><a href='/paper/abc123'>Graph Neural Networks for Drug Discovery</a></h3>
        <div data-testid='authors'><a href='/author/Alice'>Alice Smith</a><a href='/author/Bob'>Bob Jones</a></div>
        <p data-testid='snippet'>We propose a novel GNN architecture for drug-target prediction.</p>
        <span data-testid='year'>2023</span>
    </article>
    <article data-testid='search-result'>
        <h3><a href='/paper/def456'>Traffic Forecasting with Deep Learning</a></h3>
        <div data-testid='authors'><a href='/author/Carol'>Carol Lee</a></div>
        <p data-testid='snippet'>A deep learning approach to urban traffic forecasting.</p>
        <span data-testid='year'>2024</span>
    </article>
    </body></html>
    """
    stealthy_cls = _build_s2_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html, url="https://www.semanticscholar.org/search?q=test")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_semanticscholar_html("GNN", limit=5)
    assert len(items) == 2

    item = items[0]
    assert item["title"] == "Graph Neural Networks for Drug Discovery"
    assert item["url"] == "https://www.semanticscholar.org/paper/abc123"
    assert item["provider"] == "scrapling_s2"
    assert item["source"] == "scrapling_s2_search"
    assert item["category"] == "literature"
    assert item["publish_year"] == 2023
    assert "Alice Smith" in item["authors"]
    assert "Bob Jones" in item["authors"]
    assert "novel GNN" in item["content"]


@pytest.mark.asyncio
async def test_search_semanticscholar_html_skips_non_s2_urls(monkeypatch):
    """Should only accept semanticscholar.org URLs — other domains are filtered."""
    from app.providers.scrapling_search import search_semanticscholar_html

    html = """
    <html><body>
    <article data-testid='search-result'>
        <h3><a href='/paper/good'>Good S2 Paper</a></h3>
        <div data-testid='authors'><a>Alice</a></div>
        <p data-testid='snippet'>abstract</p>
    </article>
    <article data-testid='search-result'>
        <h3><a href='https://example.com/other'>Wrong URL</a></h3>
        <div data-testid='authors'><a>Bob</a></div>
        <p data-testid='snippet'>other</p>
    </article>
    </body></html>
    """
    stealthy_cls = _build_s2_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html, url="https://www.semanticscholar.org/search?q=test")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_semanticscholar_html("test", limit=5)
    # Only the S2 paper should pass the URL filter.
    assert len(items) == 1
    assert "semanticscholar.org" in items[0]["url"]
    assert items[0]["url"] == "https://www.semanticscholar.org/paper/good"


@pytest.mark.asyncio
async def test_search_semanticscholar_html_returns_empty_on_failure(fake_s2_scrapling_minimal):
    """Network failure should return empty list, not raise."""
    from app.providers.scrapling_search import search_semanticscholar_html

    fake_s2_scrapling_minimal["stealthy_cls"].fetch.side_effect = Exception("cloudflare block")

    items = await search_semanticscholar_html("test", limit=5)
    assert items == []


@pytest.fixture
def fake_s2_scrapling_minimal(monkeypatch):
    """Minimal S2 fake — no parsed HTML. Use for failure / error path tests."""
    fake_module = MagicMock()
    fake_stealthy_cls = MagicMock()
    fake_fetcher_cls = MagicMock()
    fake_selector_cls = MagicMock()
    fake_module.StealthyFetcher = fake_stealthy_cls
    fake_module.Fetcher = fake_fetcher_cls
    fake_module.Selector = fake_selector_cls
    monkeypatch.setitem(sys.modules, "scrapling", fake_module)
    return {
        "stealthy_cls": fake_stealthy_cls,
        "fetcher_cls": fake_fetcher_cls,
        "selector_cls": fake_selector_cls,
    }


@pytest.mark.asyncio
async def test_search_semanticscholar_html_extracts_year_from_text(monkeypatch):
    """Year regex should match 19xx/20xx in any of the year-selector candidates."""
    from app.providers.scrapling_search import search_semanticscholar_html

    html = """
    <html><body>
    <article data-testid='search-result'>
        <h3><a href='/paper/yr'>Year Test Paper</a></h3>
        <div data-testid='authors'></div>
        <p data-testid='snippet'></p>
        <span data-testid='year'>Published 2022 in NeurIPS</span>
    </article>
    </body></html>
    """
    stealthy_cls = _build_s2_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html, url="https://www.semanticscholar.org/search?q=test")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_semanticscholar_html("test", limit=5)
    assert len(items) == 1
    assert items[0]["publish_year"] == 2022


# --- GitHub scraper -------------------------------------------------------


class FakeGithubCssChain:
    """Chain supporting `.first` plus iteration over multiple repo blocks."""

    def __init__(self, nodes=None):
        self._nodes = nodes or []
        self.first = self._nodes[0] if self._nodes else None

    def get(self, *_args, **_kwargs):  # pragma: no cover - unused here
        return ""

    def extract_many(self):  # pragma: no cover - unused here
        return []

    def __iter__(self):
        return iter(self._nodes)

    def __bool__(self):
        return bool(self._nodes)

    def __len__(self):
        return len(self._nodes)

    def __getitem__(self, idx):
        return self._nodes[idx]


class FakeGithubNode:
    """Per-node stub that mimics Scrapling's node interface (text + attrib)."""

    def __init__(self, text="", attrib=None):
        self.text = text
        self.attrib = attrib or {}


def _build_github_fake_scrapling(monkeypatch, blocks):
    """blocks: list of dicts with keys: title, href, desc (optional).

    Builds a Selector stub that returns SearchResult-shaped blocks for
    `div.SearchResult`. The returned outer nodes have their own `.css()` method
    so per-block inner selectors work the way Scrapling's Adaptor does.
    """
    from lxml import html as lxml_html

    # Build HTML from blocks
    html_parts = ["<html><body>"]
    for b in blocks:
        title_html = b.get("title", "")
        href = b.get("href", "")
        desc = b.get("desc", "")
        html_parts.append(
            f'<div class="SearchResult">'
            f'<a class="v-align-middle" href="{href}">{title_html}</a>'
        )
        if desc:
            html_parts.append(f'<p class="color-fg-muted">{desc}</p>')
        html_parts.append("</div>")
    html_parts.append("</body></html>")
    html = "".join(html_parts)

    tree = lxml_html.fromstring(html)
    raw_blocks = tree.cssselect("div.SearchResult")

    # Create per-block outer nodes that have .css() returning inner stubs
    outer_nodes = []
    for b in raw_blocks:
        a = b.cssselect("a.v-align-middle")
        p = b.cssselect("p.color-fg-muted")
        outer = FakeGithubNode(text="")
        outer._title_text = (a[0].text_content() if a else "").strip()
        outer._title_attrib = (
            {"href": a[0].get("href", "")} if a else {}
        )
        outer._desc_text = (p[0].text_content() if p else "").strip()
        outer_nodes.append(outer)

    # Per-outer-node sub-selector mapping
    by_outer_subselector: list[dict[str, list[FakeGithubNode]]] = []
    for outer in outer_nodes:
        by_outer_subselector.append({
            "a.v-align-middle": [
                FakeGithubNode(text=outer._title_text, attrib=outer._title_attrib)
            ] if outer._title_text or outer._title_attrib else [],
            "a[data-testid='result-repo-link']": [],
            "h3 a": [],
            "p.color-fg-muted": [
                FakeGithubNode(text=outer._desc_text)
            ] if outer._desc_text else [],
            "div.SearchResult-second-line": [],
        })

    # Bind a scoped css() to each outer node so node.css(...) returns the right inner
    for idx, outer in enumerate(outer_nodes):
        outer_index = idx
        sub_map = by_outer_subselector[outer_index]

        def _make_css(sub_map=sub_map):
            def _css(selector):
                return FakeGithubCssChain(sub_map.get(selector, []))
            return _css
        outer.css = _make_css()

    by_selector = {
        "div.SearchResult": outer_nodes,
        "article.Box-row": [],
        "[data-testid='results-list'] > div": [],
    }

    def _css_factory(selector):
        if selector in by_selector:
            return FakeGithubCssChain(by_selector.get(selector, []))
        return FakeGithubCssChain([])

    fake_module = MagicMock()
    fake_stealthy_cls = MagicMock()
    fake_fetcher_cls = MagicMock()
    fake_selector_cls = MagicMock()

    def _selector_factory(content=None, url=None):
        instance = MagicMock()
        instance.css.side_effect = _css_factory
        return instance

    fake_selector_cls.side_effect = _selector_factory
    fake_module.StealthyFetcher = fake_stealthy_cls
    fake_module.Fetcher = fake_fetcher_cls
    fake_module.Selector = fake_selector_cls
    monkeypatch.setitem(sys.modules, "scrapling", fake_module)
    return fake_stealthy_cls


@pytest.mark.asyncio
async def test_search_github_html_parses_repo_blocks(monkeypatch):
    """search_github_html should return GitHub repo-shaped items."""
    from app.providers.scrapling_search import search_github_html

    stealthy_cls = _build_github_fake_scrapling(
        monkeypatch,
        [
            {
                "title": "owner / test-repo",
                "href": "/owner/test-repo",
                "desc": "A test repository for unit testing",
            }
        ],
    )
    fake_page = FakePage("<html></html>")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_github_html("test", limit=5)
    assert len(items) == 1
    item = items[0]
    assert item["title"] == "owner / test-repo"
    assert item["url"] == "https://github.com/owner/test-repo"
    assert item["provider"] == "scrapling_github"
    assert item["source"] == "scrapling_github_search"
    assert item["category"] == "code"
    assert item["authors"] == "owner"
    assert item["content"] == "A test repository for unit testing"


@pytest.mark.asyncio
async def test_search_github_html_skips_user_pages(monkeypatch):
    """User profile URLs (no /repo) should be filtered out."""
    from app.providers.scrapling_search import search_github_html

    stealthy_cls = _build_github_fake_scrapling(
        monkeypatch,
        [
            {
                "title": "Some User",
                "href": "/someuser",
                "desc": "profile page",
            }
        ],
    )
    fake_page = FakePage("<html></html>")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_github_html("user", limit=5)
    assert items == []  # user pages don't have a repo path


@pytest.mark.asyncio
async def test_search_github_html_returns_empty_on_failure(fake_scrapling):
    """Network failure should return empty list, not raise."""
    from app.providers.scrapling_search import search_github_html

    fake_scrapling["stealthy_cls"].fetch.side_effect = Exception("bot check")
    items = await search_github_html("test", limit=5)
    assert items == []


@pytest.mark.asyncio
async def test_search_github_html_uses_stealthy_fetcher(fake_scrapling):
    """GitHub has anti-bot — must use StealthyFetcher, not plain Fetcher."""
    from app.providers.scrapling_search import search_github_html

    fake_page = FakePage("<html><body></body></html>")
    fake_scrapling["stealthy_cls"].fetch.return_value = fake_page

    await search_github_html("pytorch", limit=3)

    # Verify StealthyFetcher was invoked
    assert fake_scrapling["stealthy_cls"].fetch.called
    # And plain Fetcher was NOT
    assert not fake_scrapling["fetcher_cls"].get.called


@pytest.mark.asyncio
async def test_search_github_html_handles_multiple_blocks(monkeypatch):
    """Should respect limit and return multiple valid repos."""
    from app.providers.scrapling_search import search_github_html

    blocks = [
        {"title": f"owner{i} / repo{i}", "href": f"/owner{i}/repo{i}",
         "desc": f"description {i}"}
        for i in range(5)
    ]
    stealthy_cls = _build_github_fake_scrapling(monkeypatch, blocks)
    fake_page = FakePage("<html></html>")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_github_html("repos", limit=3)
    assert len(items) == 3
    for i, item in enumerate(items):
        assert item["title"] == f"owner{i} / repo{i}"
        assert item["url"] == f"https://github.com/owner{i}/repo{i}"
        assert item["authors"] == f"owner{i}"
        assert item["relevance_score"] >= 0.1


# --- Kaggle scraper -------------------------------------------------------


class FakeKaggleNode:
    """Per-node stub mimicking Scrapling's element wrapper for Kaggle cards."""

    def __init__(self, text="", attrib=None):
        self.text = text
        self.attrib = attrib or {}
        self._children_map: dict[str, str] = {}

    def css(self, selector):
        # The implementation calls `node.css("h2, h3, h4").first` and `node.css("p").first`.
        chain = MagicMock()
        if selector in ("h2, h3, h4", "p"):
            child_text = self._children_map.get(selector, "")
            chain.first = FakeKaggleNode(text=child_text) if child_text else None
        else:
            chain.first = None
        return chain


class FakeKaggleCssChain:
    """Chain with iterable semantics matching Scrapling."""

    def __init__(self, nodes=None):
        self._nodes = nodes or []
        self.first = self._nodes[0] if self._nodes else None

    def __iter__(self):
        return iter(self._nodes)

    def __bool__(self):
        return bool(self._nodes)

    def __getitem__(self, i):
        return self._nodes[i]

    def __len__(self):
        return len(self._nodes)


def _build_kaggle_fake_scrapling(monkeypatch, html):
    """Inject a Selector stub that parses Kaggle HTML into dataset card blocks.

    Each `a[href*='/datasets/']` becomes a node. Supports nested h2/h3/h4 and p.
    """
    from lxml import html as lxml_html

    tree = lxml_html.fromstring(html)
    blocks = tree.cssselect("a[href*='/datasets/']")
    if not blocks:
        blocks = tree.cssselect("div.dataset-item") or tree.cssselect("article.sc-fzqMdJ")

    nodes = []
    for block in blocks:
        href = block.get("href", "") if hasattr(block, "get") else ""
        title_text = (block.text_content() or "").strip() if hasattr(block, "text_content") else ""
        node = FakeKaggleNode(text=title_text, attrib={"href": href})
        children_map: dict[str, str] = {}
        for sel in ("h2", "h3", "h4"):
            try:
                hits = block.cssselect(sel)
            except Exception:
                hits = []
            if hits:
                children_map["h2, h3, h4"] = (hits[0].text_content() or "").strip()
                break
        try:
            p_hits = block.cssselect("p")
        except Exception:
            p_hits = []
        if p_hits:
            children_map["p"] = (p_hits[0].text_content() or "").strip()
        node._children_map = children_map
        nodes.append(node)

    fake_module = MagicMock()
    fake_stealthy_cls = MagicMock()
    fake_fetcher_cls = MagicMock()
    fake_selector_cls = MagicMock()

    def _selector_factory(content=None, url=None):
        instance = MagicMock()

        def _css(selector):
            if selector in (
                "a[href*='/datasets/']",
                "div.dataset-item",
                "article.sc-fzqMdJ",
            ):
                return FakeKaggleCssChain(nodes)
            return FakeKaggleCssChain([])

        instance.css.side_effect = _css
        return instance

    fake_selector_cls.side_effect = _selector_factory
    fake_module.StealthyFetcher = fake_stealthy_cls
    fake_module.Fetcher = fake_fetcher_cls
    fake_module.Selector = fake_selector_cls
    monkeypatch.setitem(sys.modules, "scrapling", fake_module)
    return fake_stealthy_cls


@pytest.mark.asyncio
async def test_search_kaggle_html_parses_dataset_cards(monkeypatch):
    """Should extract title/url/provider from Kaggle dataset anchor cards."""
    from app.providers.scrapling_search import search_kaggle_html

    html = """
    <html><body>
    <a href="/datasets/owner/test-dataset">Test Dataset</a>
    <a href="/datasets/owner2/dataset2">Dataset Two description text</a>
    </body></html>
    """
    stealthy_cls = _build_kaggle_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html, url="https://www.kaggle.com/search?q=test")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_kaggle_html("test", limit=5)
    assert len(items) == 2
    urls = {item["url"] for item in items}
    assert "https://www.kaggle.com/datasets/owner/test-dataset" in urls
    assert "https://www.kaggle.com/datasets/owner2/dataset2" in urls
    for item in items:
        assert item["provider"] == "scrapling_kaggle"
        assert item["source"] == "scrapling_kaggle_search"
        assert item["category"] == "dataset"


@pytest.mark.asyncio
async def test_search_kaggle_html_dedupes_repeated_urls(monkeypatch):
    """Same URL appearing twice in HTML should only yield one result."""
    from app.providers.scrapling_search import search_kaggle_html

    html = """
    <html><body>
    <a href="/datasets/owner/test-dataset">First</a>
    <a href="/datasets/owner/test-dataset">Second (same)</a>
    </body></html>
    """
    stealthy_cls = _build_kaggle_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html, url="https://www.kaggle.com/search?q=test")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_kaggle_html("test", limit=5)
    assert len(items) == 1


@pytest.mark.asyncio
async def test_search_kaggle_html_returns_empty_on_failure(fake_scrapling):
    """Network failure should return empty list, not raise."""
    from app.providers.scrapling_search import search_kaggle_html

    fake_scrapling["stealthy_cls"].fetch.side_effect = Exception("bot blocked")
    items = await search_kaggle_html("test", limit=5)
    assert items == []


@pytest.mark.asyncio
async def test_search_kaggle_html_normalizes_absolute_urls(monkeypatch):
    """Already-absolute kaggle.com dataset URLs should be accepted and stripped of query params."""
    from app.providers.scrapling_search import search_kaggle_html

    html = """
    <html><body>
    <a href="https://www.kaggle.com/datasets/owner/test?param=foo">Test Dataset</a>
    </body></html>
    """
    stealthy_cls = _build_kaggle_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html, url="https://www.kaggle.com/search?q=test")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_kaggle_html("test", limit=5)
    assert len(items) == 1
    assert items[0]["url"] == "https://www.kaggle.com/datasets/owner/test"


@pytest.mark.asyncio
async def test_search_kaggle_html_rejects_non_kaggle_urls(monkeypatch):
    """Anchors pointing outside kaggle.com should be filtered out."""
    from app.providers.scrapling_search import search_kaggle_html

    html = """
    <html><body>
    <a href="https://example.com/datasets/foo">Wrong Domain</a>
    <a href="/datasets/owner/legit">Legit Kaggle Dataset</a>
    </body></html>
    """
    stealthy_cls = _build_kaggle_fake_scrapling(monkeypatch, html)
    fake_page = FakePage(html, url="https://www.kaggle.com/search?q=test")
    stealthy_cls.fetch.return_value = fake_page

    items = await search_kaggle_html("test", limit=5)
    assert len(items) == 1
    assert "kaggle.com/datasets/" in items[0]["url"]
