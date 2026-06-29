"use client"

import { useEffect, useRef, useState } from "react"
import { Loader2, MapPin, Search } from "lucide-react"
import { LeafletMap, type MapLayer } from "./leaflet-map"
import { LayerPicker, type PickerLayer } from "./layer-picker"
import { ETOSHA, type GeoLayerTag } from "./etosha-preset"
import {
  geoBuffer,
  geoFeatures,
  geoHealth,
  geoPlaces,
  type GeoPlaceResult,
} from "@/lib/api"

/**
 * GeoPanel — the workbench's Geo Workshop activity.
 *
 * State machine (kept simple for the v1 cut):
 *   1. Pick a place (preset button or text-search + geocode dropdown)
 *   2. Toggle which OSM tag layers to load (chip picker)
 *   3. Hit Run → spinner while each layer streams from `/geo/features`
 *   4. Layers render on the Leaflet map as they arrive
 *
 * Buffer / spatial-join tools attach once a layer is selected.
 * Out of scope for v1: routing, isochrones, exporting the GeoJSON
 * to the project's file tree.
 */

interface GeoPanelProps {
  projectId: string
  capabilities?: string[]
}

export function GeoPanel({ projectId: _projectId, capabilities: _capabilities }: GeoPanelProps) {
  // Backend availability — pings `/geo/health` once on mount.
  const [available, setAvailable] = useState<boolean | null>(null)
  useEffect(() => {
    geoHealth()
      .then((r) => setAvailable(r.available))
      .catch(() => setAvailable(false))
  }, [])

  // ── place picker ────────────────────────────────────────
  const [placeQuery, setPlaceQuery] = useState("")
  const [places, setPlaces] = useState<GeoPlaceResult[]>([])
  const [selectedPlace, setSelectedPlace] = useState<GeoPlaceResult | null>(null)
  const [searching, setSearching] = useState(false)
  // Debounce timer for the geocode lookup as the user types.
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    if (!placeQuery.trim()) {
      setPlaces([])
      return
    }
    if (searchTimer.current) clearTimeout(searchTimer.current)
    searchTimer.current = setTimeout(async () => {
      setSearching(true)
      try {
        const results = await geoPlaces(placeQuery.trim(), 5)
        setPlaces(results)
      } catch (err) {
        // Geocode failures are non-fatal — Nominatim rate-limit or
        // unreachable. Clear the list silently.
        console.warn("geocode failed:", err)
        setPlaces([])
      } finally {
        setSearching(false)
      }
    }, 300)
    return () => {
      if (searchTimer.current) clearTimeout(searchTimer.current)
    }
  }, [placeQuery])

  // ── layer picker ────────────────────────────────────────
  const [layers, setLayers] = useState<PickerLayer[]>(() =>
    ETOSHA.tags.map((tag) => ({ ...tag, enabled: true })),
  )
  const applyPreset = () => {
    setLayers(ETOSHA.tags.map((tag) => ({ ...tag, enabled: true })))
    setSelectedPlace({
      id: "preset-etosha",
      display_name: ETOSHA.name,
      lat: -19.0,
      lon: 16.4,
      bbox: null,
      geojson: null,
    })
    setPlaceQuery(ETOSHA.name)
    setMapLayers([])
    setBufferLayer(null)
  }

  // ── map layers (rendered state) ──────────────────────
  const [mapLayers, setMapLayers] = useState<MapLayer[]>([])
  const [running, setRunning] = useState(false)
  const [runError, setRunError] = useState<string | null>(null)

  // ── buffer tool ─────────────────────────────────────────
  const [bufferDistance, setBufferDistance] = useState<number>(ETOSHA.defaultBuffer_m)
  const [bufferLayer, setBufferLayer] = useState<MapLayer | null>(null)

  // Run the actual fetch — fires one `/geo/features` call per enabled
  // tag, accumulates results into `mapLayers`.
  const runQuery = async () => {
    if (!selectedPlace) return
    const enabled = layers.filter((l) => l.enabled)
    if (enabled.length === 0) return
    setRunning(true)
    setRunError(null)
    setMapLayers([])
    setBufferLayer(null)
    try {
      const newLayers: MapLayer[] = []
      for (const layer of enabled) {
        const result = await geoFeatures({
          place: selectedPlace.display_name,
          tags: { [layer.key]: layer.value as unknown },
        })
        newLayers.push({
          key: layer.key,
          name: layer.label,
          color: layer.color,
          featureCollection: result,
        })
      }
      setMapLayers(newLayers)
    } catch (err) {
      console.error("geo features fetch failed:", err)
      const msg = err instanceof Error ? err.message : String(err)
      setRunError(msg)
    } finally {
      setRunning(false)
    }
  }

  // Apply buffer to the first enabled layer that returned features.
  const applyBuffer = async () => {
    const target = mapLayers[0]
    if (!target) return
    try {
      const result = await geoBuffer({
        features: target.featureCollection,
        distance_m: bufferDistance,
      })
      setBufferLayer({
        key: `buffer-${bufferDistance}-${target.key}`,
        name: `${target.name} buffer (${bufferDistance} m)`,
        color: "#d4a574",
        featureCollection: result.buffered,
      })
    } catch (err) {
      console.error("buffer failed:", err)
    }
  }

  // ── map center / bbox from selected place ─────────────
  const mapCenter = selectedPlace && selectedPlace.lat != null && selectedPlace.lon != null
    ? [selectedPlace.lat, selectedPlace.lon] as [number, number]
    : undefined
  const mapBounds = selectedPlace?.bbox ?? undefined

  const displayedLayers = bufferLayer ? [...mapLayers, bufferLayer] : mapLayers

  // ── render ──────────────────────────────────────────
  if (available === false) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-essay-bg p-6 text-center text-sm text-essay-text-muted">
        <MapPin className="h-8 w-8 text-essay-text-faint" />
        <p>
          The Geo Workshop needs{" "}
          <code className="rounded bg-essay-code-bg px-1.5 py-0.5 text-essay-accent">
            osmnx[neighbors]==2.1.0
          </code>{" "}
          installed in the sidecar.
        </p>
        <p className="text-xs text-essay-text-faint">
          Run{" "}
          <code className="rounded bg-essay-code-bg px-1.5 py-0.5">
            pip install osmnx[neighbors]==2.1.0 shapely pyogrio
          </code>{" "}
          in <code>src-tauri/sidecar</code> and restart the app.
        </p>
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col bg-essay-bg">
      {/* Top controls */}
      <div className="flex flex-col gap-3 border-b border-essay-border p-3">
        <div className="flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-essay-text-faint" />
            <input
              type="text"
              value={placeQuery}
              onChange={(e) => {
                setPlaceQuery(e.target.value)
                setSelectedPlace(null)
              }}
              placeholder="Search a place (e.g. Berkeley, CA)…"
              className="w-full rounded border border-essay-border bg-essay-bg py-1.5 pl-7 pr-3 text-sm text-essay-text placeholder:text-essay-text-faint focus:border-essay-accent focus:outline-none"
            />
            {searching && (
              <Loader2 className="absolute right-2 top-1/2 h-3 w-3 -translate-y-1/2 animate-spin text-essay-text-faint" />
            )}
            {places.length > 0 && !selectedPlace && (
              <ul className="absolute left-0 right-0 top-full z-20 mt-1 max-h-60 overflow-y-auto rounded border border-essay-border bg-essay-bg p-1 text-sm shadow-lg">
                {places.map((p) => (
                  <li key={p.id}>
                    <button
                      type="button"
                      className="block w-full truncate rounded px-2 py-1 text-left text-essay-text hover:bg-essay-bg-hover"
                      onClick={() => {
                        setSelectedPlace(p)
                        setPlaceQuery(p.display_name)
                      }}
                    >
                      {p.display_name}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <button
            type="button"
            onClick={applyPreset}
            className="rounded border border-essay-accent/40 bg-essay-accent/10 px-3 py-1.5 text-xs font-medium text-essay-accent hover:bg-essay-accent/20"
            title={ETOSHA.description}
          >
            Etosha preset
          </button>
        </div>

        <div className="flex items-center gap-2">
          <span className="text-[10px] font-semibold uppercase tracking-wider text-essay-text-faint">
            Layers
          </span>
          <div className="flex-1">
            <LayerPicker
              layers={layers}
              onChange={setLayers}
              onAddCustomTag={() => {
                // Lightweight add: drop in a `building` tag chip. The
                // user can fine-tune in the API later if they need
                // specific filters.
                setLayers([
                  ...layers,
                  {
                    key: `building-${Date.now()}`,
                    label: "Buildings",
                    color: "#ba68c8",
                    value: true,
                    enabled: true,
                  },
                ])
              }}
            />
          </div>
          <button
            type="button"
            disabled={!selectedPlace || layers.every((l) => !l.enabled) || running}
            onClick={runQuery}
            className="inline-flex items-center gap-1.5 rounded bg-essay-accent px-4 py-1.5 text-xs font-medium text-essay-primary-foreground hover:bg-essay-accent-hover disabled:cursor-not-allowed disabled:opacity-40"
          >
            {running && <Loader2 className="h-3 w-3 animate-spin" />}
            {running ? "Loading OSM…" : "Run"}
          </button>
        </div>

        {/* Buffer control row */}
        <div className="flex items-center gap-2">
          <span className="text-[10px] font-semibold uppercase tracking-wider text-essay-text-faint">
            Buffer
          </span>
          <input
            type="number"
            value={bufferDistance}
            min={50}
            step={100}
            onChange={(e) => setBufferDistance(Number(e.target.value))}
            className="w-24 rounded border border-essay-border bg-essay-bg px-2 py-0.5 text-xs text-essay-text focus:border-essay-accent focus:outline-none"
          />
          <span className="text-xs text-essay-text-muted">m</span>
          <button
            type="button"
            disabled={mapLayers.length === 0}
            onClick={applyBuffer}
            className="rounded border border-essay-border px-2 py-0.5 text-xs text-essay-text-muted hover:border-essay-text-muted disabled:cursor-not-allowed disabled:opacity-40"
            title="Buffer the first loaded layer's features by the distance above"
          >
            Buffer first layer
          </button>
        </div>

        {runError && (
          <div className="rounded border border-red-700/40 bg-red-950/30 px-3 py-2 text-xs text-red-300">
            {runError}
          </div>
        )}
      </div>

      {/* Map fills the rest */}
      <div className="relative flex-1">
        {available === null ? (
          <div className="flex h-full items-center justify-center text-xs text-essay-text-faint">
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            Checking sidecar…
          </div>
        ) : (
          <LeafletMap
            layers={displayedLayers}
            center={mapCenter}
            bounds={mapBounds}
          />
        )}
        {displayedLayers.length > 0 && (
          <div className="pointer-events-none absolute bottom-2 left-2 rounded bg-essay-bg/80 px-2 py-1 text-[10px] text-essay-text-muted backdrop-blur-sm">
            {displayedLayers.length} layer{displayedLayers.length === 1 ? "" : "s"} ·{" "}
            {displayedLayers.reduce(
              (n, l) => n + l.featureCollection.features.length,
              0,
            )}{" "}
            feature{displayedLayers.length === 1 ? "" : "s"}
          </div>
        )}
      </div>
    </div>
  )
}
