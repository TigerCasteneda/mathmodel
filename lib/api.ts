let API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:3001"

let apiBasePromise: Promise<string> | null = null

async function getApiBase(): Promise<string> {
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    if (!apiBasePromise) {
      const { getServerPort } = await import("@/lib/tauri-api")
      apiBasePromise = getServerPort().then((port) => `http://127.0.0.1:${port}`)
    }
    return apiBasePromise
  }
  return API_BASE
}

// Some routes live on a different port even inside Tauri: the embedded
// Rust/Node backend handles auth/files/AI, but the Geo Workshop's `/geo/*`
// endpoints live on the Python sidecar (FastAPI). Detect that prefix and
// address the sidecar directly so the request doesn't 404 against the
// embedded backend. Returns null when the sidecar isn't running yet, in
// which case the caller should fail with a friendly message.
let sidecarBasePromise: Promise<string | null> | null = null

async function getSidecarBase(): Promise<string | null> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return null
  }
  if (!sidecarBasePromise) {
    sidecarBasePromise = (async () => {
      const { getSidecarPort } = await import("@/lib/tauri-api")
      const port = await getSidecarPort()
      return port > 0 ? `http://127.0.0.1:${port}` : null
    })()
  }
  return sidecarBasePromise
}

function pathGoesToSidecar(path: string): boolean {
  return path.startsWith("/geo/")
}

export async function getWebSocketBase(): Promise<string> {
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    const { getServerPort } = await import("@/lib/tauri-api")
    const port = await getServerPort()
    return `ws://127.0.0.1:${port}`
  }
  if (process.env.NEXT_PUBLIC_WS_URL) return process.env.NEXT_PUBLIC_WS_URL
  if (API_BASE.startsWith("https://")) return API_BASE.replace(/^https:\/\//, "wss://")
  if (API_BASE.startsWith("http://")) return API_BASE.replace(/^http:\/\//, "ws://")
  return "ws://localhost:3001"
}

let tokenStore: string | null = null
let refreshTokenStore: string | null = null

export function setTokens(token: string, refreshToken: string) {
  tokenStore = token
  refreshTokenStore = refreshToken
  if (typeof window !== "undefined") {
    localStorage.setItem("auth_token", token)
    localStorage.setItem("auth_refresh", refreshToken)
  }
}

export function loadTokens() {
  if (typeof window !== "undefined") {
    tokenStore = localStorage.getItem("auth_token")
    refreshTokenStore = localStorage.getItem("auth_refresh")
  }
}

export function clearTokens() {
  tokenStore = null
  refreshTokenStore = null
  if (typeof window !== "undefined") {
    localStorage.removeItem("auth_token")
    localStorage.removeItem("auth_refresh")
  }
}

export function getToken() {
  if (!tokenStore) loadTokens()
  return tokenStore
}

/**
 * Decode the payload of a JWT without verifying its signature. Sufficient for
 * reading the `sub` (user id), `email`, and `user_metadata.display_name`
 * claims that Supabase-style auth tokens carry. We trust the issuer: the
 * token was minted by our backend after credential validation, and any
 * forged token would fail on the next API call regardless of what we read
 * out of it here.
 *
 * Returns null on malformed input (e.g. three-part token split, base64
 * decode failure, JSON parse failure) so callers can fall through to a
 * generic profile instead of crashing.
 */
export function decodeJwtClaims(token: string | null | undefined): Record<string, unknown> | null {
  if (!token) return null
  try {
    const parts = token.split(".")
    if (parts.length < 2) return null
    let payload = parts[1].replace(/-/g, "+").replace(/_/g, "/")
    const pad = payload.length % 4
    if (pad) payload += "=".repeat(4 - pad)
    if (typeof atob !== "function") return null
    const json = atob(payload)
    const parsed = JSON.parse(json)
    return typeof parsed === "object" && parsed !== null ? parsed : null
  } catch {
    return null
  }
}

async function refreshAccessToken(): Promise<boolean> {
  if (!refreshTokenStore) return false
  try {
    const base = await getApiBase()
    const res = await fetch(`${base}/auth/refresh`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: refreshTokenStore }),
    })
    if (!res.ok) return false
    const data = await res.json()
    setTokens(data.token, data.refresh_token)
    return true
  } catch {
    return false
  }
}

export async function apiFetch<T = unknown>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  const token = getToken()
  const headers: Record<string, string> = {
    ...(options.headers as Record<string, string> || {}),
  }
  if (token) {
    headers["Authorization"] = `Bearer ${token}`
  }

  // Some routes live on the Python sidecar, not the embedded backend.
  // Detect the prefix and address the right port.
  let base: string
  if (pathGoesToSidecar(path)) {
    const sidecar = await getSidecarBase()
    if (!sidecar) {
      throw new Error("Geo sidecar is not running. Restart the app to retry.")
    }
    base = sidecar
  } else {
    base = await getApiBase()
  }

  const fullUrl = `${base}${path}`
  let res: Response
  try {
    res = await fetch(fullUrl, { ...options, headers })
  } catch (err) {
    // fetch throws "Failed to fetch" (TypeError) for network-level
    // failures: connection refused, DNS, CSP block, CORS preflight, etc.
    // Without the URL the message is useless — surface it so callers
    // (and the Geo Workshop banner) can diagnose.
    const reason = err instanceof Error ? err.message : String(err)
    throw new Error(`fetch ${fullUrl} failed: ${reason}`)
  }

  if (res.status === 401 && refreshTokenStore && !pathGoesToSidecar(path)) {
    const refreshed = await refreshAccessToken()
    if (refreshed) {
      headers["Authorization"] = `Bearer ${getToken()}`
      res = await fetch(fullUrl, { ...options, headers })
    }
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: "unknown error" }))
    throw new Error(err.error || `HTTP ${res.status}`)
  }

  return res.json()
}

export interface UserProfile {
  id: string
  email: string
  display_name: string
}

export interface Project {
  id: string
  name: string
  owner_id: string
  role: string
  capabilities: string
  created_at: number
  updated_at: number
}

export type ProjectRole = "owner" | "editor" | "viewer"

export type ProjectCapability =
  | "files.read"
  | "files.write"
  | "ai.read"
  | "ai.write"
  | "workspace.sync"
  | "members.manage"
  | "invites.manage"
  | "screen.share"
  | "screen.view"

export const ALL_PROJECT_CAPABILITIES: ProjectCapability[] = [
  "files.read",
  "files.write",
  "ai.read",
  "ai.write",
  "workspace.sync",
  "members.manage",
  "invites.manage",
  "screen.share",
  "screen.view",
]

export function parseCapabilities(raw?: string | null): ProjectCapability[] {
  if (!raw) return []
  try {
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed)
      ? parsed.filter((cap): cap is ProjectCapability => ALL_PROJECT_CAPABILITIES.includes(cap))
      : []
  } catch {
    return []
  }
}

export interface ProjectMember {
  user_id: string
  email: string
  display_name: string
  role: ProjectRole
  capabilities: string
  joined_at: number
}

export interface InviteCodeResponse {
  code: string
  expires_at?: number | null
  role: ProjectRole
  capabilities: ProjectCapability[]
}

export interface ProjectInvite {
  id: string
  code: string
  max_uses: number
  used_count: number
  expires_at?: number | null
  created_at: number
  role: ProjectRole
  capabilities?: string | null
}

export async function getProject(projectId: string): Promise<Project> {
  return apiFetch<Project>(`/projects/${projectId}`)
}

export async function listProjectMembers(projectId: string): Promise<ProjectMember[]> {
  return apiFetch<ProjectMember[]>(`/projects/${projectId}/members`)
}

export async function updateProjectMember(
  projectId: string,
  userId: string,
  input: { role?: ProjectRole; capabilities?: ProjectCapability[] },
): Promise<ProjectMember> {
  return apiFetch<ProjectMember>(`/projects/${projectId}/members/${userId}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  })
}

export async function removeProjectMember(
  projectId: string,
  userId: string,
): Promise<{ removed: boolean }> {
  return apiFetch<{ removed: boolean }>(`/projects/${projectId}/members/${userId}`, {
    method: "DELETE",
  })
}

export async function listProjectInvites(projectId: string): Promise<ProjectInvite[]> {
  return apiFetch<ProjectInvite[]>(`/projects/${projectId}/invites`)
}

export async function createProjectInvite(
  projectId: string,
  input: {
    role?: ProjectRole
    capabilities?: ProjectCapability[]
    max_uses?: number
    expires_in_hours?: number
  } = {},
): Promise<InviteCodeResponse> {
  return apiFetch<InviteCodeResponse>(`/projects/${projectId}/invite`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  })
}

export async function revokeProjectInvite(projectId: string): Promise<{ revoked: number }> {
  return apiFetch<{ revoked: number }>(`/projects/${projectId}/invite`, {
    method: "DELETE",
  })
}

export async function joinProjectByCode(code: string): Promise<{ project_id: string }> {
  return apiFetch<{ project_id: string }>("/projects/join", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code }),
  })
}

export interface ProjectFileTreeItem {
  id: string
  name: string
  type: "file" | "folder"
  zone: string
  updated_at: number
  children?: ProjectFileTreeItem[]
}

export interface ProjectFileNode {
  id: string
  project_id: string
  parent_id?: string | null
  name: string
  type: "file" | "folder"
  mime_type?: string | null
  size: number
  storage_path?: string | null
  zone: string
  created_at: number
  updated_at: number
}

export interface ProjectFileContent {
  file_id: string
  content: string
  updated_at: number
}

export async function listProjectTree(projectId: string): Promise<ProjectFileTreeItem[]> {
  return apiFetch<ProjectFileTreeItem[]>(`/projects/${projectId}/tree`)
}

export async function getProjectFileContent(
  projectId: string,
  fileId: string,
): Promise<ProjectFileContent> {
  return apiFetch<ProjectFileContent>(`/projects/${projectId}/files/${fileId}/content`)
}

export async function createProjectFile(
  projectId: string,
  input: {
    name: string
    type: "file" | "folder"
    parent_id?: string | null
    zone?: string
  },
): Promise<ProjectFileNode> {
  return apiFetch<ProjectFileNode>(`/projects/${projectId}/files`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      name: input.name,
      type: input.type,
      parent_id: input.parent_id ?? null,
      zone: input.zone ?? "code",
    }),
  })
}

export async function updateProjectFileContent(
  projectId: string,
  fileId: string,
  content: string,
  expectedUpdatedAt?: number,
): Promise<ProjectFileContent> {
  return apiFetch<ProjectFileContent>(`/projects/${projectId}/files/${fileId}/content`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      content,
      expected_updated_at: expectedUpdatedAt,
    }),
  })
}

export async function deleteProjectFile(
  projectId: string,
  fileId: string,
): Promise<{ deleted: boolean }> {
  return apiFetch<{ deleted: boolean }>(`/projects/${projectId}/files/${fileId}`, {
    method: "DELETE",
  })
}

// ── Research / Search ──

export interface ArenaCard {
  file_id: string
  title: string
  card_type: "formula" | "finding" | "assumption" | "decision" | "note" | string
  tags: string[]
  aliases: string[]
  status: string
  links: string[]
  backlinks: string[]
  unresolved_links: string[]
  content: string
  updated_at: number
  /** `user_id` of the user who originally created this card. `null` for
   * legacy rows predating the authorship migration; render as "Unknown". */
  created_by: string | null
  /** `user_id` of the user who most recently saved the card. Mirrors
   * `updated_at`. `null` for legacy rows. */
  last_edited_by: string | null
}

export interface ArenaIndex {
  cards: ArenaCard[]
  unresolved_links: string[]
}

export interface CreateArenaCardInput {
  card_type: "formula" | "finding" | "assumption" | "decision" | "note" | string
  title: string
  tags?: string[]
  body?: string
}

export interface UpdateArenaCardInput {
  content: string
  expected_updated_at?: number
}

export interface AppendArenaLogResponse {
  file_id: string
  content: string
  updated_at: number
}

export async function listArenaCards(projectId: string): Promise<ArenaCard[]> {
  return apiFetch<ArenaCard[]>(`/projects/${projectId}/arena/cards`)
}

export async function createArenaCard(projectId: string, input: CreateArenaCardInput): Promise<ArenaCard> {
  return apiFetch<ArenaCard>(`/projects/${projectId}/arena/cards`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  })
}

export async function updateArenaCard(projectId: string, cardId: string, input: UpdateArenaCardInput): Promise<ArenaCard> {
  return apiFetch<ArenaCard>(`/projects/${projectId}/arena/cards/${cardId}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  })
}

export async function appendArenaLog(projectId: string, message: string): Promise<AppendArenaLogResponse> {
  return apiFetch<AppendArenaLogResponse>(`/projects/${projectId}/arena/log`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ message }),
  })
}

export async function getArenaIndex(projectId: string): Promise<ArenaIndex> {
  return apiFetch<ArenaIndex>(`/projects/${projectId}/arena/index`)
}

// ── Arena Chat ──

export interface ChatMessage {
  id: string
  project_id: string
  user_id: string
  display_name: string
  content: string
  content_type: "text" | "file" | "system" | string
  reply_to_id?: string | null
  file_id?: string | null
  file_name?: string | null
  file_mime?: string | null
  content_attributes: Record<string, unknown>
  status: "sent" | "failed" | "sending"
  echo_id?: string | null
  replied_to?: {
    user_id: string
    display_name: string
    content_preview: string
  } | null
  created_at: number
}

export interface OnlineUser {
  user_id: string
  display_name: string
}

export interface ChatHistoryPage {
  messages: ChatMessage[]
  has_more: boolean
  next_cursor?: number | null
}

export interface FetchChatHistoryParams {
  before?: number
  limit?: number
}

export async function fetchChatHistory(
  projectId: string,
  params?: FetchChatHistoryParams,
): Promise<ChatHistoryPage> {
  const sp = new URLSearchParams()
  if (params?.before != null) sp.set("before", String(params.before))
  if (params?.limit != null) sp.set("limit", String(params.limit))
  const qs = sp.toString()
  return apiFetch<ChatHistoryPage>(`/projects/${projectId}/arena/chat/messages${qs ? `?${qs}` : ""}`)
}

export async function uploadChatFile(projectId: string, file: File): Promise<ProjectFileNode> {
  const formData = new FormData()
  formData.append("file", file)
  const token = getToken()
  const base = await getApiBase()
  const res = await fetch(`${base}/projects/${projectId}/files/upload`, {
    method: "POST",
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    body: formData,
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: "upload failed" }))
    throw new Error(err.error || `HTTP ${res.status}`)
  }
  return res.json()
}

// ── Search ──

export interface SearchResultItem {
  title: string
  url: string
  content: string
  authors?: string
  publish_year?: number
  keywords?: string
  relevance_score: number
}

export interface SearchResponse {
  query: string
  results: SearchResultItem[]
}

export interface SaveItemInput {
  title: string
  url: string
  content: string
  category: string
  summary?: string
  authors?: string
  publish_year?: number
  keywords?: string
  methodology?: string
  key_parameters?: string
  ai_relevance?: string
  relevance_score?: number
  bibtex?: string
  raw_json?: Record<string, unknown>
}

export interface ResearchItem {
  id: string
  project_id: string
  created_by: string
  source: string
  category: string
  url: string
  title?: string
  summary?: string
  authors?: string
  publish_year?: number
  keywords?: string
  notes?: string
  relevance_score: number
  cloud_file_id?: string
  methodology: string
  key_parameters: string
  ai_relevance: string
  raw_json: string
  created_at: number
  updated_at: number
}

export interface SaveItemsResponse {
  saved: number
  items: ResearchItem[]
  files_created: number
  warnings?: string[]
}

export async function researchSearch(
  projectId: string,
  query: string,
  category: string,
  maxResults = 20
): Promise<SearchResponse> {
  void projectId
  void query
  void category
  void maxResults
  throw new Error("Research search now runs through native Modeler AI chat.")
}

export async function saveResearchItems(
  projectId: string,
  items: SaveItemInput[]
): Promise<SaveItemsResponse> {
  return apiFetch<SaveItemsResponse>("/research/items", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ project_id: projectId, items }),
  })
}

export async function listResearchItems(
  projectId: string,
  category?: string,
  sort = "created_at",
  order = "desc",
  limit = 50,
  offset = 0
): Promise<ResearchItem[]> {
  const params = new URLSearchParams({ project_id: projectId, sort, order, limit: String(limit), offset: String(offset) })
  if (category) params.set("category", category)
  return apiFetch<ResearchItem[]>(`/research/items?${params.toString()}`)
}

export async function getResearchItem(itemId: string): Promise<ResearchItem> {
  return apiFetch<ResearchItem>(`/research/items/${itemId}`)
}

export async function updateResearchItem(
  itemId: string,
  data: { notes?: string; category?: string }
): Promise<ResearchItem> {
  return apiFetch<ResearchItem>(`/research/items/${itemId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  })
}

export async function deleteResearchItem(itemId: string): Promise<{ deleted: boolean }> {
  return apiFetch<{ deleted: boolean }>(`/research/items/${itemId}`, {
    method: "DELETE",
  })
}

// ─── Geo Workshop ────────────────────────────────────────
//
// Thin wrappers around the sidecar's `/geo/*` endpoints (osmnx-powered).
// Both the FastAPI server and the sidecar share the same port
// (`getServerPort()`), so these route through the existing `apiFetch`.
//
// A 503 from `/geo/health` means osmnx / shapely / geopandas failed to
// load inside the sidecar — usually a missing pip dependency. The UI
// pings `/geo/health` once on mount and surfaces a banner if false.

export interface GeoPlaceResult {
  id: string
  display_name: string
  lat: number | null
  lon: number | null
  /** [west, south, east, north] in WGS84, or null if Nominatim returned a point. */
  bbox: [number, number, number, number] | null
  /** Polygon (preferred) or point geometry as a GeoJSON Geometry object. */
  geojson: { type: string; coordinates: unknown } | null
}

export interface GeoStats {
  n?: number
  m?: number
  k_avg?: number
  edge_length_total?: number
  intersection_count?: number
  streets_per_node_avg?: number
  circuity_avg?: number
  [k: string]: unknown
}

export async function geoHealth(): Promise<{ available: boolean }> {
  return apiFetch<{ available: boolean }>("/geo/health")
}

export async function geoPlaces(
  q: string,
  limit = 5
): Promise<GeoPlaceResult[]> {
  const res = await apiFetch<{ results: GeoPlaceResult[] }>(
    "/geo/places",
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ q, limit }),
    }
  )
  return res.results
}

export async function geoFeatures(input: {
  place?: string
  /** [west, south, east, north] in WGS84. Mutually exclusive with `place`. */
  bbox?: [number, number, number, number]
  /** OSM tags dict, e.g. `{ natural: "water" }` or `{ highway: true }`. */
  tags?: Record<string, unknown>
}): Promise<GeoJSON.FeatureCollection> {
  return apiFetch<GeoJSON.FeatureCollection>("/geo/features", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  })
}

export interface GeoGraphResult {
  graph: GeoJSON.FeatureCollection
  stats: GeoStats
}

export async function geoGraph(input: {
  place?: string
  bbox?: [number, number, number, number]
  /** "drive" | "walk" | "bike" | "all" — defaults to "drive". */
  network_type?: string
}): Promise<GeoGraphResult> {
  return apiFetch<GeoGraphResult>("/geo/graph", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ network_type: "drive", ...input }),
  })
}

export interface GeoBufferResult {
  buffered: GeoJSON.FeatureCollection
}

export async function geoBuffer(input: {
  features: GeoJSON.FeatureCollection
  /** Buffer radius in meters. */
  distance_m: number
  /** If true, all output polygons are merged into one. */
  dissolve?: boolean
}): Promise<GeoBufferResult> {
  return apiFetch<GeoBufferResult>("/geo/buffer", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  })
}

export interface GeoSpatialJoinResult {
  joined: GeoJSON.FeatureCollection
  /** polygon feature index → count of points inside */
  counts: Record<string, number>
}

export async function geoSpatialJoin(input: {
  points: GeoJSON.FeatureCollection
  polygons: GeoJSON.FeatureCollection
  /** "intersects" | "contains" | "within" | "dwithin" — defaults to "intersects". */
  predicate?: string
}): Promise<GeoSpatialJoinResult> {
  return apiFetch<GeoSpatialJoinResult>("/geo/spatial_join", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ predicate: "intersects", ...input }),
  })
}

export interface GeoNetworkStatsResult {
  stats: GeoStats
}

export async function geoNetworkStats(input: {
  place?: string
  bbox?: [number, number, number, number]
}): Promise<GeoNetworkStatsResult> {
  return apiFetch<GeoNetworkStatsResult>("/geo/stats", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  })
}
