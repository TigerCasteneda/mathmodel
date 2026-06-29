"""Geo-workshop provider — thin async wrappers over osmnx.

The frontend's Geo Workshop panel calls these via the sidecar's `/geo/*`
HTTP endpoints. Each function returns a dict shaped for the React
side: GeoJSON FeatureCollection(s), bbox, identifiers — small
enough to JSON-serialize without extra paging.

All osmnx calls are blocking (Overpass / Nominatim requests can take
5–30 s), so every entry point wraps the actual `ox.*` call in
`asyncio.to_thread()` to keep FastAPI's event loop responsive.

Configuration:
- `ox.settings.http_user_agent` is set once at module import time, per
  OpenStreetMap's tile/Overpass usage policy. Apps hitting OSM should
  identify themselves.
- `ox.settings.cache_folder` defaults to a sidecar-local cache so
  repeated calls don't hammer Overpass. Override via the
  `MODELER_GEO_CACHE` env var if the sidecar's working dir is ephemeral.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
from typing import Any

# All osmnx calls are sync, blocking. `asyncio.to_thread` keeps the
# FastAPI event loop responsive while Overpass queries land.
#
# Network policy: OSM's public Overpass + Nominatim endpoints require
# a real User-Agent. Default osmnx User-Agent identifies the
# library only — override with our app identity per OSM ToS.
_DEFAULT_UA = os.environ.get(
    "MODELER_GEO_UA",
    "ModelerAI/1.0 (+https://modeler.ai) Python/osmnx",
)
_CACHE_DIR = os.environ.get("MODELER_GEO_CACHE", "/tmp/modeler-geo-cache")

try:
    import osmnx as ox
    import geopandas as gpd
    from shapely.geometry import shape as _shape
    from shapely.ops import unary_union

    ox.settings.http_user_agent = _DEFAULT_UA
    ox.settings.cache_folder = _CACHE_DIR
    # osmnx's Nominatim helper sleeps 1s between requests by default;
    # leave the rate-limit checks on so we don't get 429s.
    ox.settings.overpass_rate_limit = True
    _OSMNX_OK = True
except Exception as _exc:  # pragma: no cover — exercised when deps missing
    # We allow the sidecar to start even if osmnx failed to import so
    # the rest of the app still serves academic search / etc. The /geo/*
    # routes will respond 503 via _require_osmnx() below.
    _OSMNX_OK = False
    _IMPORT_ERROR = repr(_exc)

log = logging.getLogger(__name__)


__all__ = [
    "geocode_places",
    "fetch_features",
    "fetch_graph",
    "buffer_features",
    "spatial_join",
    "network_stats",
    "is_available",
]


# ── helpers ────────────────────────────────────────────────────────────


def is_available() -> bool:
    """Whether osmnx + its transitive deps loaded successfully.

    Used by `app/main.py` to gate the /geo/* routes with HTTP 503
    instead of a stack trace if the sidecar was started without the
    geo extras installed.
    """
    return _OSMNX_OK


def _require_osmnx() -> None:
    if not _OSMNX_OK:
        hint = globals().get("_IMPORT_ERROR", "import failed")
        raise RuntimeError(
            f"osmnx is not available ({hint}); "
            "install with `pip install osmnx[neighbors]==2.1.0`"
        )


def _gdf_to_geojson(gdf: "gpd.GeoDataFrame") -> dict[str, Any]:
    """Convert a GeoDataFrame to a GeoJSON FeatureCollection dict.

    Drops the GeoDataFrame index/CRS metadata to keep payloads small;
    the geometry carries enough context for the React map to render.
    """
    return json.loads(gdf.to_json())


def _maybe_tags(tags: dict | None) -> dict | None:
    """Normalize the OSM tag filter dict from the JSON request body.

    Accepts `{key: value}` where value is `true` (bool) | str | list[str].
    Returns None if input is empty so osmnx uses its default behavior.
    """
    if not tags:
        return None
    return {k: v for k, v in tags.items() if v is not None}


# ── public surface ────────────────────────────────────────────────────


async def geocode_places(q: str, limit: int = 5) -> list[dict[str, Any]]:
    """Forward geocode a free-text query.

    Returns a list of matches, each `{id, display_name, bbox, lat, lon, geojson}`.
    `bbox` is `[west, south, east, north]` in WGS84.
    """
    _require_osmnx()

    def _run() -> list[dict[str, Any]]:
        gdf = ox.geocode_to_gdf(q)
        if gdf.empty:
            return []
        # gdf columns: geometry, bbox (tuple), lat, lon, display_name,
        # osmid, class, type, place_rank, etc.
        out = []
        for _, row in gdf.head(limit).iterrows():
            geom = row.geometry
            bbox = row.get("bbox") if hasattr(row, "get") else None
            out.append({
                "id": str(row.get("osmid", row.name)),
                "display_name": str(row.get("display_name", q)),
                "lat": float(row.get("lat")) if row.get("lat") is not None else None,
                "lon": float(row.get("lon")) if row.get("lon") is not None else None,
                "bbox": list(bbox) if bbox is not None else None,
                "geojson": geom.__geo_interface__ if geom is not None else None,
            })
        return out

    return await asyncio.to_thread(_run)


async def fetch_features(
    place: str | None,
    bbox: list[float] | None,
    tags: dict[str, Any],
) -> dict[str, Any]:
    """Fetch OSM features inside a place polygon or bounding box.

    Exactly one of `place` / `bbox` must be provided. Returns
    `{features: GeoJSON FeatureCollection}`.
    """
    _require_osmnx()
    cleaned_tags = _maybe_tags(tags)

    def _run() -> dict[str, Any]:
        if place:
            if not cleaned_tags:
                gdf = ox.features_from_place(place)
            else:
                gdf = ox.features_from_place(place, tags=cleaned_tags)
        elif bbox and len(bbox) == 4:
            west, south, east, north = bbox
            if not cleaned_tags:
                gdf = ox.features_from_bbox((west, south, east, north))
            else:
                gdf = ox.features_from_bbox((west, south, east, north), tags=cleaned_tags)
        else:
            raise ValueError("either place or bbox (4-tuple) is required")
        return {"features": _gdf_to_geojson(gdf)}

    return await asyncio.to_thread(_run)


async def fetch_graph(
    place: str | None,
    bbox: list[float] | None,
    network_type: str = "drive",
) -> dict[str, Any]:
    """Download OSM street network as a GeoJSON LineString FeatureCollection.

    Also returns the standard `basic_stats` dict alongside the geometry
    so the React panel can show "n nodes / m edges / km of road".
    """
    _require_osmnx()

    def _run() -> dict[str, Any]:
        if place:
            G = ox.graph_from_place(place, network_type=network_type, simplify=True)
        elif bbox and len(bbox) == 4:
            G = ox.graph_from_bbox(tuple(bbox), network_type=network_type, simplify=True)
        else:
            raise ValueError("either place or bbox (4-tuple) is required")
        nodes_gdf, edges_gdf = ox.convert.graph_to_gdfs(G, nodes=False)
        # Drop the row-index (edge key) from the JSON to keep things tidy.
        edges_gdf = edges_gdf.reset_index(drop=True)
        stats = ox.stats.basic_stats(G)
        return {
            "graph": _gdf_to_geojson(edges_gdf),
            "stats": {k: stats.get(k) for k in (
                "n", "m", "k_avg", "edge_length_total",
                "intersection_count", "streets_per_node_avg",
                "circuity_avg",
            )},
        }

    return await asyncio.to_thread(_run)


async def buffer_features(
    features: dict[str, Any],
    distance_m: float,
    dissolve: bool = False,
) -> dict[str, Any]:
    """Buffer every feature in a GeoJSON FeatureCollection by `distance_m` meters.

    `dissolve=True` unions all polygons into one (useful for "everything
    within 5 km of any waterhole").

    Returns `{buffered: GeoJSON Polygon FeatureCollection}`.
    """
    _require_osmnx()

    def _run() -> dict[str, Any]:
        src = gpd.GeoDataFrame.from_features(features)
        # Project to a metric CRS so the buffer is in meters, then
        # back to WGS84 so the front-end Leaflet layer renders natively.
        # 3857 (Web Mercator) preserves shape well for small areas and is
        # supported by every GeoJSON consumer.
        projected = src.to_crs(3857)
        buffered = projected.buffer(distance_m)
        if dissolve:
            buffered = gpd.GeoSeries(unary_union(buffered))
            buffered = gpd.GeoDataFrame(geometry=buffered)
        buffered = buffered.to_crs(4326)
        return {"buffered": _gdf_to_geojson(buffered)}

    return await asyncio.to_thread(_run)


async def spatial_join(
    points: dict[str, Any],
    polygons: dict[str, Any],
    predicate: str = "intersects",
) -> dict[str, Any]:
    """Spatial-join a points FeatureCollection against a polygons FC.

    Returns `{joined: GeoJSON, counts: {feature_index: int}}` so the
    React panel can badge each polygon with the number of points it
    contains (e.g. "waterhole has 12 wildlife observations").
    """
    _require_osmnx()

    def _run() -> dict[str, Any]:
        pts = gpd.GeoDataFrame.from_features(points).reset_index(drop=True)
        pgs = gpd.GeoDataFrame.from_features(polygons).reset_index(drop=True)
        joined = gpd.sjoin(pts, pgs, how="left", predicate=predicate)
        # counts: polygon-index → point-count
        counts_series = joined.groupby(joined.index_right).size()
        counts = {int(idx): int(c) for idx, c in counts_series.items()}
        # Carry the polygon index onto each point so the FE can render
        # joined = points tagged with their containing polygon's idx.
        joined["polygon_idx"] = joined["index_right"].fillna(-1).astype(int)
        # Drop the sjoin internals we don't want to ship.
        keep_cols = [c for c in ("polygon_idx", "geometry") if c in joined.columns]
        joined = joined[keep_cols]
        return {"joined": _gdf_to_geojson(joined), "counts": counts}

    return await asyncio.to_thread(_run)


async def network_stats(
    place: str | None,
    bbox: list[float] | None,
) -> dict[str, Any]:
    """Run `basic_stats` on a graph and return node/edge/length metrics."""
    _require_osmnx()

    def _run() -> dict[str, Any]:
        if place:
            G = ox.graph_from_place(place, network_type="drive", simplify=True)
        elif bbox and len(bbox) == 4:
            G = ox.graph_from_bbox(tuple(bbox), network_type="drive", simplify=True)
        else:
            raise ValueError("either place or bbox (4-tuple) is required")
        stats = ox.stats.basic_stats(G)
        # Restrict to a JSON-friendly subset.
        keys = (
            "n", "m", "k_avg",
            "edge_length_total", "edge_length_avg",
            "streets_per_node_avg", "streets_per_node_counts",
            "intersection_count",
            "street_length_total", "street_segment_count",
            "circuity_avg", "self_loop_proportion",
        )
        return {"stats": {k: stats.get(k) for k in keys}}

    return await asyncio.to_thread(_run)