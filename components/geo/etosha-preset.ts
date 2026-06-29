/**
 * Etosha preset — the worked example driving the Geo Workshop.
 *
 * Source: `geoworkflow.md` (Palacký University thesis: geotagged photos
 * + OSM layers to design wildlife distribution maps). The four OSM tag
 * selectors below match the layers the thesis pulled from Overpass:
 * roads, waterholes, camps, gates.
 *
 * Drives both the UI chips (labels + tag dict) and the osmnx `tags=`
 * filter on the sidecar. The default buffer radius is "everything
 * within 5 km of any waterhole" — the thesis's central hypothesis.
 */

export interface GeoLayerTag {
  /** The OSM tag key. e.g. `"highway"`, `"natural"`. */
  key: string
  /**
   * The OSM tag value. Per `ox.features_from_place`'s `tags=`
   * convention this can be:
   *   - `true`           → key exists with any value
   *   - a string         → exact value match
   *   - an array of strs → union of value matches
   */
  value: true | string | string[]
  /** Display label for the chip in the UI. */
  label: string
  /** Short hex color (or CSS var name) used for the GeoJSON layer. */
  color: string
}

export interface GeoPreset {
  name: string
  description: string
  /** OSM tag selectors to load as separate map layers. */
  tags: GeoLayerTag[]
  /** Buffer radius in meters for the "buffer waterholes" preset
   * workflow. */
  defaultBuffer_m: number
}

export const ETOSHA: GeoPreset = {
  name: "Etosha National Park, Namibia",
  description:
    "Worked example — a wildlife habitat survey. Waterholes, roads, camps, and gates from OpenStreetMap over the national park polygon. Buffer the waterholes by 5 km to get the candidate animal-zone hull.",
  tags: [
    { key: "highway", value: true, label: "Roads", color: "#9aa0a6" },
    { key: "natural", value: "water", label: "Water", color: "#4fc3f7" },
    { key: "tourism", value: "camp_site", label: "Camps", color: "#d4a574" },
    { key: "barrier", value: "gate", label: "Gates", color: "#ef5350" },
  ],
  defaultBuffer_m: 5000,
}
