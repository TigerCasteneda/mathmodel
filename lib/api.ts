const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:3001"

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

async function refreshAccessToken(): Promise<boolean> {
  if (!refreshTokenStore) return false
  try {
    const res = await fetch(`${API_BASE}/auth/refresh`, {
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

  let res = await fetch(`${API_BASE}${path}`, { ...options, headers })

  if (res.status === 401 && refreshTokenStore) {
    const refreshed = await refreshAccessToken()
    if (refreshed) {
      headers["Authorization"] = `Bearer ${getToken()}`
      res = await fetch(`${API_BASE}${path}`, { ...options, headers })
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
  created_at: number
  updated_at: number
}

// ── Research / Search ──

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
  relevance_score?: number
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
  raw_json: string
  created_at: number
  updated_at: number
}

export interface SaveItemsResponse {
  saved: number
  items: ResearchItem[]
  files_created: number
}

export async function researchSearch(
  projectId: string,
  query: string,
  category: string,
  maxResults = 20
): Promise<SearchResponse> {
  return apiFetch<SearchResponse>("/research/search", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ project_id: projectId, query, category, max_results: maxResults }),
  })
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
