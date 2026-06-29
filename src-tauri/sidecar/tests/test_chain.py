"""Chain (integration) tests for the sidecar.

These tests exercise the **real** chain — actual Chromium launch via
patchright, real network requests, real HTML parsing — rather than the
mocked-out paths used in `test_stealth.py` / `test_scrapling_search.py`.

Each test skips cleanly when:
  - patchright / Chromium isn't installed (CI / fresh dev boxes)
  - the network can't reach the target host (corporate proxies, DNS)

Mark with `pytest.mark.chain` so they can be selected/deselected:
    pytest -m chain                              # only chain tests
    pytest -m "not chain"                        # only unit tests
    pytest                                      # both
"""

from __future__ import annotations

import asyncio
import socket
import sys
import time
from pathlib import Path
from typing import Any

import pytest

# Make the app package importable when running pytest from repo root or
# from this directory.
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


# ---------------------------------------------------------------------------
# Skip predicates
# ---------------------------------------------------------------------------

def _chromium_available() -> tuple[bool, str]:
    """Return (available, message). Probe patchright's Chromium install path."""
    try:
        from patchright.sync_api import sync_playwright  # type: ignore

        with sync_playwright() as p:
            path = p.chromium.executable_path
            if path and Path(path).exists():
                return True, f"chromium at {path}"
            return False, f"chromium executable_path missing: {path!r}"
    except Exception as exc:  # pragma: no cover - depends on env
        return False, f"patchright probe failed: {type(exc).__name__}: {exc}"


def _network_reachable(host: str = "github.com", port: int = 443, timeout: float = 3.0) -> bool:
    """Cheap TCP probe — no data transfer, just SYN/ACK."""
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


# Apply at module level so pytest collects them as skipped rather than errored.
_chromium_ok, _chromium_msg = _chromium_available()
needs_chromium = pytest.mark.skipif(
    not _chromium_ok,
    reason=f"patchright Chromium not available — {_chromium_msg}",
)
needs_github_net = pytest.mark.skipif(
    not _network_reachable("github.com"),
    reason="github.com:443 unreachable from this host",
)
needs_arxiv_net = pytest.mark.skipif(
    not _network_reachable("arxiv.org"),
    reason="arxiv.org:443 unreachable from this host",
)
needs_zenodo_net = pytest.mark.skipif(
    not _network_reachable("zenodo.org"),
    reason="zenodo.org:443 unreachable from this host",
)


# ---------------------------------------------------------------------------
# 1. StealthyFetcher really launches Chromium + fetches a real URL
# ---------------------------------------------------------------------------

@pytest.mark.chain
@needs_chromium
@needs_github_net
@pytest.mark.asyncio
async def test_stealthy_get_real_chromium_fetch():
    """End-to-end: patchright launches Chromium → GETs GitHub → returns Response.

    This is the lowest-level chain test. If Chromium can't launch, or the
    network blocks StealthyFetcher, this test fails.
    """
    from app.providers.stealth import _stealthy_get

    page = await _stealthy_get("https://github.com")
    assert page is not None, "StealthyFetcher returned None"
    html = getattr(page, "html_content", "") or ""
    assert len(html) > 1000, f"GitHub homepage too small ({len(html)} bytes)"
    assert "github" in html.lower(), "Response doesn't look like GitHub"


# ---------------------------------------------------------------------------
# 2. search_github_html end-to-end with real network + real selectors
# ---------------------------------------------------------------------------

@pytest.mark.chain
@needs_chromium
@needs_github_net
@pytest.mark.asyncio
async def test_search_github_html_returns_real_repos():
    """Full chain: StealthyFetcher → github.com HTML → Selector → list of repos."""
    from app.providers.scrapling_search import search_github_html

    items = await search_github_html("transformer", limit=3)
    assert len(items) >= 1, f"expected at least 1 repo, got 0 (HTML structure may have changed)"
    for item in items[:3]:
        assert item["url"].startswith("https://github.com/"), f"bad url: {item['url']}"
        # Must have owner/repo (exactly 2 path segments)
        path = item["url"].replace("https://github.com/", "").strip("/")
        assert len(path.split("/")) == 2, f"not a repo url: {item['url']}"
        assert "/" in item["title"], f"title should be 'owner / repo': {item['title']!r}"
        assert item["category"] == "code"
        assert item["provider"] == "scrapling_github"


@pytest.mark.chain
@needs_chromium
@needs_github_net
@pytest.mark.asyncio
async def test_search_github_html_includes_description():
    """Verify desc extraction works on the new GitHub HTML (Content-module class)."""
    from app.providers.scrapling_search import search_github_html

    items = await search_github_html("transformer", limit=2)
    assert items, "no items returned — can't verify description"
    # Most transformer repos have non-trivial descriptions. If description
    # is empty, it likely fell back to title (means Content-module selector
    # stopped matching).
    for item in items[:2]:
        content = item.get("content") or ""
        # description should not just be the title
        assert content != item["title"] or len(content) > 30, (
            f"description fell back to title: {content!r} (HTML structure changed?)"
        )


# ---------------------------------------------------------------------------
# 3. arxiv / zenodo plain-Fetcher chain (no Chromium needed but real network)
# ---------------------------------------------------------------------------

@pytest.mark.chain
@needs_arxiv_net
@pytest.mark.asyncio
async def test_search_arxiv_html_returns_real_papers():
    """Plain Fetcher chain (no Chromium) — should work without patchright."""
    from app.providers.scrapling_search import search_arxiv_html

    items = await search_arxiv_html("neural ODE", limit=3)
    assert items, "arxiv returned 0 — may be blocked"
    for item in items[:3]:
        assert "arxiv.org" in item["url"], f"non-arxiv url: {item['url']}"
        assert item["title"], "empty title"


@pytest.mark.chain
@needs_zenodo_net
@pytest.mark.asyncio
async def test_zenodo_rest_api_returns_real_datasets():
    """Zenodo dataset search via the InvenioRDM REST API.

    Replaces the legacy HTML scrape (``scrapling_search.search_zenodo_html``),
    which broke when Zenodo migrated to an InvenioRDM SPA — search results
    are JS-rendered and the SSR HTML contains 0 records.
    """
    from app.providers.zenodo import search as zenodo_search

    items = await zenodo_search("climate", limit=3)
    assert items, "zenodo REST API returned 0 — may be blocked"
    for item in items[:3]:
        # URL is either doi.org/<doi> or zenodo.org/records/<id>
        assert ("zenodo.org" in item.url) or ("doi.org" in item.url), f"bad url: {item.url}"
        assert item.category == "dataset"
        assert item.title, "empty title"


@pytest.mark.chain
@needs_zenodo_net
@pytest.mark.asyncio
async def test_zenodo_endpoint_over_http_returns_real_datasets():
    """Full HTTP chain: sidecar /search/datasets/scrapling/zenodo via uvicorn."""
    import subprocess

    import httpx

    port = _free_port()
    sidecar_dir = Path(__file__).resolve().parent.parent
    proc = subprocess.Popen(
        [sys.executable, "run.py"],
        cwd=str(sidecar_dir),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        port_actual: int | None = None
        deadline = time.time() + 15.0
        while time.time() < deadline:
            line = proc.stdout.readline()
            if not line:
                break
            if line.startswith("SIDECAR_PORT="):
                port_actual = int(line.strip().split("=", 1)[1])
                break
        assert port_actual is not None, (
            f"sidecar didn't print SIDECAR_PORT within 15s. stderr:\n"
            f"{proc.stderr.read() if proc.stderr else '(no stderr)'}"
        )

        r = httpx.post(
            f"http://127.0.0.1:{port_actual}/search/datasets/scrapling/zenodo",
            json={"query": "climate", "limit": 3},
            timeout=30.0,
        )
        assert r.status_code == 200, f"endpoint failed: {r.status_code} {r.text[:500]}"
        body = r.json()
        assert len(body["items"]) >= 1, (
            f"zenodo endpoint returned 0 items over real HTTP. body={body}"
        )
        first = body["items"][0]
        assert ("zenodo.org" in first["url"]) or ("doi.org" in first["url"])
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


# ---------------------------------------------------------------------------
# 4. _wrap_scrapling_search timing heuristic (unit, but live process)
# ---------------------------------------------------------------------------

@pytest.mark.chain
def test_wrap_scrapling_search_flags_slow_zero_item_response():
    """0 items + elapsed > 20s should produce a timeout-likely warning.

    This is the heuristic in main.py:_wrap_scrapling_search. It's a unit
    test but kept here in the chain file because it's the contract the
    Rust caller depends on.
    """
    from app.main import _wrap_scrapling_search

    # Simulate a 25-second call that returned nothing (StealthyFetcher timeout)
    t0 = time.monotonic() - 25.0
    resp = _wrap_scrapling_search([], t0, "github")
    assert resp.warning is not None, "expected warning for slow + empty"
    assert "timeout" in resp.warning.lower()
    assert "github" in resp.warning
    assert resp.items == []


@pytest.mark.chain
def test_wrap_scrapling_search_no_warning_for_fast_zero_items():
    """0 items + elapsed < 2s is selector rot, not a timeout — no warning."""
    from app.main import _wrap_scrapling_search

    t0 = time.monotonic() - 0.5
    resp = _wrap_scrapling_search([], t0, "github")
    assert resp.warning is None, f"unexpected warning: {resp.warning}"
    assert resp.items == []


@pytest.mark.chain
def test_wrap_scrapling_search_no_warning_when_items_present():
    """Got items — don't add a warning even if it took a while."""
    from app.main import _wrap_scrapling_search

    t0 = time.monotonic() - 30.0
    resp = _wrap_scrapling_search(
        [{"title": "foo/bar", "url": "https://github.com/foo/bar", "content": "",
          "provider": "scrapling_github", "source": "scrapling_github_search",
          "category": "code"}],
        t0,
        "github",
    )
    assert resp.warning is None
    assert len(resp.items) == 1


# ---------------------------------------------------------------------------
# 5. Live FastAPI sidecar endpoint (start uvicorn, hit over real HTTP)
# ---------------------------------------------------------------------------

def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@pytest.mark.chain
@needs_chromium
@needs_github_net
def test_sidecar_http_endpoint_live_github_search():
    """Start uvicorn in a subprocess, hit /search/code/scrapling/github over HTTP.

    This catches issues the unit tests miss: import errors in main.py,
    CORS, JSON serialization, route mounting, etc.
    """
    import subprocess

    import httpx

    port = _free_port()
    sidecar_dir = Path(__file__).resolve().parent.parent
    proc = subprocess.Popen(
        [sys.executable, "run.py"],
        cwd=str(sidecar_dir),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env={**__import__("os").environ},
    )
    try:
        # Wait for the SIDECAR_PORT=... line on stdout
        port_actual: int | None = None
        deadline = time.time() + 15.0
        while time.time() < deadline:
            line = proc.stdout.readline()
            if not line:
                break
            if line.startswith("SIDECAR_PORT="):
                port_actual = int(line.strip().split("=", 1)[1])
                break
        assert port_actual is not None, (
            f"sidecar didn't print SIDECAR_PORT within 15s. stderr:\n"
            f"{proc.stderr.read() if proc.stderr else '(no stderr)'}"
        )

        # /health should pass first
        r = httpx.get(f"http://127.0.0.1:{port_actual}/health", timeout=5.0)
        assert r.status_code == 200, f"/health failed: {r.status_code} {r.text}"

        # Now the real chain test
        r = httpx.post(
            f"http://127.0.0.1:{port_actual}/search/code/scrapling/github",
            json={"query": "transformer", "limit": 2},
            timeout=70.0,
        )
        assert r.status_code == 200, f"endpoint failed: {r.status_code} {r.text[:500]}"
        body = r.json()
        assert "items" in body, f"missing items field: {body}"
        assert isinstance(body["items"], list)
        # warning may be None or a string
        assert "warning" in body
        assert len(body["items"]) >= 1, (
            f"github scrapling returned 0 items over real HTTP — chain broken. "
            f"warning={body.get('warning')!r}"
        )
        # Spot-check first item shape
        first = body["items"][0]
        assert first["url"].startswith("https://github.com/")
        assert "/" in first["title"]
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


# ---------------------------------------------------------------------------
# 6. Patchright Chromium really installed (sanity check for the dev env)
# ---------------------------------------------------------------------------

@pytest.mark.chain
def test_patchright_chromium_installed():
    """Make sure patchright's Chromium binary actually exists.

    This is a sentinel: if it fails, all the @needs_chromium tests above
    are skipped silently. Running this in isolation surfaces the missing
    binary loudly.
    """
    available, msg = _chromium_available()
    assert available, (
        f"patchright Chromium not installed — run `py -3 -m patchright install chromium`. "
        f"Probe: {msg}"
    )