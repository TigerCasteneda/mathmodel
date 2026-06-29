"""Tests for app/providers/geo.py and the /geo/* FastAPI routes.

Strategy: real osmnx behind the `ox.settings.cache_folder` (set to a
per-test temp dir) keeps heavy Overpass queries fast on repeat runs.
The unit-level mocks in earlier drafts were fragile and shed in
practice — drop them.

What's covered:
- `is_available()` says True if osmnx imported
- `_require_osmnx()` raises with actionable message
- Route contracts: 400 on missing place/bbox, 503 when unavailable
- `_maybe_tags` strips None values

What's NOT covered (slow + network-bound, skip in unit suite):
- Live geocode (`ox.geocode_to_gdf` hits Nominatim) — would need
  recording tests / VCR-style fixtures. Out of scope for v1.
- Live feature fetch — same reason.

Run: `py -3 -m pytest tests/test_geo.py -v`
"""

from __future__ import annotations

import pytest

from app.providers import geo


# ── sanity: osmnx + geopandas + shapely all loaded ──────────────────


def test_geo_provider_reports_available():
    assert geo.is_available() is True


def test_require_osmnx_passes_when_available():
    # Should not raise.
    geo._require_osmnx()


def test_require_osmnx_raises_with_actionable_hint(monkeypatch):
    saved = geo._OSMNX_OK
    monkeypatch.setattr(geo, "_OSMNX_OK", False, raising=False)
    try:
        with pytest.raises(RuntimeError, match="pip install"):
            geo._require_osmnx()
    finally:
        geo._OSMNX_OK = saved


def test_maybe_tags_strips_none():
    assert geo._maybe_tags({"k": "v", "x": None}) == {"k": "v"}
    assert geo._maybe_tags({}) is None
    assert geo._maybe_tags(None) is None


# ── FastAPI route contract tests ─────────────────────────────────────


@pytest.fixture
def client():
    """Bare TestClient — the app.main module already imports osmnx for
    real in this environment, so routes are wired and runnable."""
    from fastapi.testclient import TestClient

    from app import main as app_main

    return TestClient(app_main.app)


def test_geo_health_endpoint(client):
    r = client.get("/geo/health")
    assert r.status_code == 200
    body = r.json()
    assert "available" in body


def test_geo_features_endpoint_rejects_no_args(client):
    """Live osmnx would happily geocode — but with no args the validator
    rejects before any network traffic."""
    r = client.post("/geo/places", json={"q": ""})
    assert r.status_code == 400


def test_geo_features_endpoint_rejects_no_place_or_bbox(client):
    r = client.post("/geo/features", json={"tags": {"highway": True}})
    assert r.status_code == 400


def test_geo_buffer_endpoint_rejects_missing_features(client):
    r = client.post(
        "/geo/buffer", json={"distance_m": 1000}
    )
    assert r.status_code == 400


def test_geo_buffer_endpoint_rejects_non_featurecollection(client):
    r = client.post(
        "/geo/buffer",
        json={"features": {"type": "Point"}, "distance_m": 1000},
    )
    assert r.status_code == 400


def test_geo_endpoints_503_when_unavailable(monkeypatch):
    saved = geo._OSMNX_OK
    monkeypatch.setattr(geo, "_OSMNX_OK", False, raising=False)
    try:
        from fastapi.testclient import TestClient

        from app import main as app_main

        c = TestClient(app_main.app)
        r = c.post("/geo/places", json={"q": "Etosha"})
        assert r.status_code == 503, r.text
        assert "osmnx" in r.text.lower()
    finally:
        geo._OSMNX_OK = saved