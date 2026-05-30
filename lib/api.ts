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
