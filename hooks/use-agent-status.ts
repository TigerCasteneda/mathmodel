"use client"

import { useState, useEffect } from "react"

export type AgentStatus = "ready" | "disconnected" | "connecting"

export function useAgentStatus(projectId: string): AgentStatus {
  const [status, setStatus] = useState<AgentStatus>("connecting")

  useEffect(() => {
    const token =
      typeof window !== "undefined"
        ? localStorage.getItem("auth_token")
        : null
    if (!token) {
      setStatus("disconnected")
      return
    }

    const wsUrl = `${process.env.NEXT_PUBLIC_WS_URL || "ws://localhost:3001"}/agent?token=${encodeURIComponent(token)}&project_id=${encodeURIComponent(projectId)}&role=frontend`
    const ws = new WebSocket(wsUrl)

    ws.onopen = () => {
      setStatus("connecting")
    }

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data)
        if (msg.type === "agent_status") {
          setStatus(msg.status === "ready" ? "ready" : "disconnected")
        }
      } catch {
        /* ignore parse errors */
      }
    }

    ws.onclose = () => {
      setStatus("disconnected")
    }

    ws.onerror = () => {
      setStatus("disconnected")
    }

    return () => {
      ws.close()
    }
  }, [projectId])

  return status
}
