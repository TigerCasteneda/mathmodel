"use client"

import { useEffect, useMemo, useState } from "react"
import {
  MapContainer,
  TileLayer,
  GeoJSON as LeafletGeoJSON,
  CircleMarker,
  Popup,
  useMap,
} from "react-leaflet"
import L from "leaflet"
import "leaflet/dist/leaflet.css"

/**
 * LeafletMap — react-leaflet 5 wrapper for the Geo Workshop panel.
 *
 * Renders an OpenStreetMap raster basemap with one `<GeoJSON>` layer per
 * `FeatureGroup` passed in. Clicking a feature pops a small info card.
 *
 * SSR note: Leaflet touches `window` at import time. The component is
 * gated on a `mounted` flag (set in `useEffect`) so the initial server
 * render renders nothing, then the client picks up Leaflet cleanly.
 *
 * Props
 * ─────
 * - `layers`  ordered list of `{key, color, featureCollection, name}`
 *   groups. Each one becomes a togglable `<GeoJSON>` overlay. Empty
 *   `layers` renders just the basemap + center marker.
 * - `center`  initial [lat, lon] if no layer has features to auto-fit on.
 * - `bounds`  optional initial `[w, s, e, n]` to set the map view.
 * - `onMapClick`  receives `{lat, lon}` when the user clicks empty
 *   basemap (used by the panel to drop a marker for spatial-join).
 */

export interface MapLayer {
  /** Unique key for React; also the toggle label. */
  key: string
  /** Human-readable name shown in the popup header. */
  name: string
  /** Stroke + fill color for the GeoJSON path style. */
  color: string
  /** A GeoJSON FeatureCollection (sent from the sidecar). */
  featureCollection: GeoJSON.FeatureCollection
}

export interface LeafletMapProps {
  layers?: MapLayer[]
  center?: [number, number]
  bounds?: [number, number, number, number]
  /** Optional marker placed at a clicked-on point. */
  clickedPoint?: { lat: number; lon: number } | null
  onMapClick?: (point: { lat: number; lon: number }) => void
}

export function LeafletMap({
  layers = [],
  center,
  bounds,
  clickedPoint,
  onMapClick,
}: LeafletMapProps) {
  // SSR safety: don't touch Leaflet until we're in the browser.
  const [mounted, setMounted] = useState(false)
  useEffect(() => {
    setMounted(true)
  }, [])

  // Stable style function — recreating per-feature kills perf.
  const styleFor = useMemo(() => {
    return (color: string) => () => ({
      color,
      weight: 1.5,
      opacity: 0.9,
      fillColor: color,
      fillOpacity: 0.18,
    })
  }, [])

  if (!mounted) {
    return <div className="h-full w-full bg-essay-bg" aria-hidden />
  }

  // Default center: first layer's first feature's coordinates, or Etosha.
  const fallbackCenter: [number, number] = center ?? [-19.0, 16.4]
  const initialBounds = bounds
    ? L.latLngBounds(
        [bounds[1], bounds[0]],
        [bounds[3], bounds[2]],
      )
    : undefined

  return (
    <MapContainer
      center={fallbackCenter}
      bounds={initialBounds}
      zoom={bounds ? undefined : 9}
      style={{ height: "100%", width: "100%" }}
      preferCanvas={true}
      className="z-0"
    >
      <TileLayer
        url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
        attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'
        maxZoom={19}
      />
      <ClickCatcher onClick={onMapClick} />
      {layers.map((layer) => (
        <LeafletGeoJSON
          key={layer.key}
          data={layer.featureCollection as GeoJSON.GeoJsonObject}
          style={styleFor(layer.color) as L.PathOptions}
          onEachFeature={(feature, leafletLayer) => {
            const props = (feature.properties ?? {}) as Record<string, unknown>
            const name =
              (props["name"] as string | undefined) ??
              (props["display_name"] as string | undefined) ??
              props["osm_id"]?.toString() ??
              "Feature"
            const tags = Object.entries(props)
              .filter(([k]) => k !== "name" && k !== "display_name")
              .slice(0, 6)
              .map(([k, v]) => `${k}=${v as string}`)
              .join(" · ")
            leafletLayer.bindPopup(
              `<strong>${name}</strong><br/>${tags || layer.name}`,
            )
          }}
        />
      ))}
      {clickedPoint && (
        <CircleMarker
          center={[clickedPoint.lat, clickedPoint.lon]}
          radius={6}
          pathOptions={{
            color: "#d4a574",
            weight: 2,
            fillColor: "#d4a574",
            fillOpacity: 0.6,
          }}
        >
          <Popup>Clicked point ({clickedPoint.lat.toFixed(4)}, {clickedPoint.lon.toFixed(4)})</Popup>
        </CircleMarker>
      )}
      {initialBounds && <FitBoundsOnLoad bounds={initialBounds} />}
    </MapContainer>
  )
}

/** Imperative shim: fit the map view to the layer's bounds once it loads. */
function FitBoundsOnLoad({ bounds }: { bounds: L.LatLngBounds }) {
  const map = useMap()
  useEffect(() => {
    map.fitBounds(bounds)
  }, [map, bounds])
  return null
}

/** Imperative shim: capture Leaflet `click` events on basemap and
 * forward `{lat, lon}` up to the parent via `onMapClick`. The
 * `useMapEvent` hook is from react-leaflet. */
function ClickCatcher({
  onClick,
}: {
  onClick?: (point: { lat: number; lon: number }) => void
}) {
  const map = useMap()
  // Direct event listener (no useMapEvent here — we want native Leaflet
  // click resolution in lat/lng).
  useEffect(() => {
    if (!onClick) return
    const handler = (e: L.LeafletMouseEvent) => {
      onClick({ lat: e.latlng.lat, lon: e.latlng.lng })
    }
    map.on("click", handler)
    return () => {
      map.off("click", handler)
    }
  }, [map, onClick])
  return null
}
