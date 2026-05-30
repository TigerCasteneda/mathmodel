"use client"

import { useState, useEffect, useCallback } from "react"
import { apiFetch, setTokens, clearTokens, loadTokens, getToken, UserProfile } from "@/lib/api"

interface AuthState {
  user: UserProfile | null
  loading: boolean
}

export function useAuth() {
  const [state, setState] = useState<AuthState>({ user: null, loading: true })

  useEffect(() => {
    loadTokens()
    const token = getToken()
    if (token) {
      apiFetch<Array<unknown>>("/projects")
        .then(() => {
          setState({ user: { id: "", email: "", display_name: "User" }, loading: false })
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
