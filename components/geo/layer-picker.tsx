"use client"

import { GeoLayerTag } from "./etosha-preset"

/**
 * LayerPicker — chip-style toggles for OSM tag selectors.
 *
 * Renders one chip per `GeoLayerTag` (e.g. "Roads", "Water", "Camps").
 * Selected chips drive `onChange(enabledKeys)` which the parent passes
 * to `/geo/features`.
 *
 * Add (+) and Remove (×) buttons let the user extend the layer set
 * beyond the preset defaults.
 */

export interface PickerLayer extends GeoLayerTag {
  /** True if the chip is currently enabled (will be requested). */
  enabled: boolean
}

export interface LayerPickerProps {
  layers: PickerLayer[]
  onChange(layers: PickerLayer[]): void
  onReset?(): void
  /** Optional inline "+ add custom tag" affordance; pass tags here. */
  onAddCustomTag?(): void
}

export function LayerPicker({
  layers,
  onChange,
  onReset,
  onAddCustomTag,
}: LayerPickerProps) {
  const toggle = (key: string) => {
    onChange(
      layers.map((layer) =>
        layer.key === key ? { ...layer, enabled: !layer.enabled } : layer,
      ),
    )
  }
  const remove = (key: string) => {
    onChange(layers.filter((layer) => layer.key !== key))
  }

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {layers.map((layer) => (
        <div
          key={layer.key}
          className={`group inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors ${
            layer.enabled
              ? "border-essay-accent/60 bg-essay-accent/10 text-essay-text"
              : "border-essay-border bg-essay-bg text-essay-text-muted hover:border-essay-text-faint"
          }`}
        >
          <button
            type="button"
            onClick={() => toggle(layer.key)}
            className="flex items-center gap-1"
            title={`tag=${layer.key}=${String(layer.value)}`}
          >
            <span
              className="inline-block h-2 w-2 rounded-full"
              style={{ backgroundColor: layer.color }}
              aria-hidden
            />
            <span>{layer.label}</span>
          </button>
          {onAddCustomTag && (
            <button
              type="button"
              onClick={() => remove(layer.key)}
              className="ml-0.5 hidden text-essay-text-faint hover:text-essay-text group-hover:inline-block"
              aria-label={`Remove ${layer.label}`}
            >
              ×
            </button>
          )}
        </div>
      ))}
      {onAddCustomTag && (
        <button
          type="button"
          onClick={onAddCustomTag}
          className="inline-flex items-center rounded-full border border-dashed border-essay-border px-2 py-0.5 text-xs text-essay-text-muted hover:border-essay-text-muted"
        >
          + tag
        </button>
      )}
      {onReset && layers.length > 0 && (
        <button
          type="button"
          onClick={onReset}
          className="text-[10px] uppercase tracking-wider text-essay-text-faint hover:text-essay-text"
        >
          reset
        </button>
      )}
    </div>
  )
}
