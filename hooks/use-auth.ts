"use client"

import { useState, useEffect, useCallback } from "react"
import {
  apiFetch,
  setTokens,
  clearTokens,
  loadTokens,
  getToken,
  decodeJwtClaims,
  UserProfile,
} from "@/lib/api"

interface AuthState {
  user: UserProfile | null
  loading: boolean
}

/**
 * Build a UserProfile from the claims embedded in a Supabase-style JWT.
 * Used on page load when we have a stored token but no fresh server
 * response (avoids an extra roundtrip just to learn who the user is).
 * Falls back to a degraded profile if any claim is missing.
 */
function profileFromToken(token: string | null): UserProfile | null {
  const claims = decodeJwtClaims(token)
  if (!claims) return null
  const id = typeof claims.sub === "string" ? claims.sub : ""
  const email = typeof claims.email === "string" ? claims.email : ""
  const meta =
    typeof claims.user_metadata === "object" && claims.user_metadata !== null
      ? (claims.user_metadata as Record<string, unknown>)
      : undefined
  const metaName = typeof meta?.display_name === "string" ? meta.display_name : undefined
  const metaFullName = typeof meta?.full_name === "string" ? meta.full_name : undefined
  const display_name = metaName || metaFullName || email.split("@")[0] || "User"
  if (!id) return null
  return { id, email, display_name }
}

export function useAuth() {
  const [state, setState] = useState<AuthState>({ user: null, loading: true })

  useEffect(() => {
    loadTokens()
    const token = getToken()
    if (token) {
      // Pull identity directly out of the stored JWT — no need to round-trip
      // through /projects just to learn who the user is. Then validate the
      // token still works against the server; on failure clear and log out.
      const fromToken = profileFromToken(token)
      apiFetch<Array<unknown>>("/projects")
        .then(() => {
          setState({ user: fromToken, loading: false })
        })
        .catch(() => {
          clearTokens()
          setState({ user: null, loading: false })
        })
    } else {
      setState({ user: null, loading: false })
    }
  }, [])

  const login = useCallback(async (email: string, password: string) => {
    const data = await apiFetch<{ token: string; refresh_token: string; user: UserProfile }>(
      "/auth/login",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, password }),
      }
    )
    setTokens(data.token, data.refresh_token)
    setState({ user: data.user, loading: false })
    return data.user
  }, [])

  const register = useCallback(async (email: string, password: string, display_name: string) => {
    const data = await apiFetch<{ token: string; refresh_token: string; user: UserProfile }>(
      "/auth/register",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, password, display_name }),
      }
    )
    setTokens(data.token, data.refresh_token)
    setState({ user: data.user, loading: false })
    return data.user
  }, [])

  const logout = useCallback(() => {
    clearTokens()
    setState({ user: null, loading: false })
  }, [])

  return { ...state, login, register, logout }
}
