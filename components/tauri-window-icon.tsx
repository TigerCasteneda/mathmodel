"use client"

import { useEffect } from "react"

import { isTauri } from "@/lib/tauri-api"

export function TauriWindowIcon() {
  useEffect(() => {
    if (!isTauri()) return

    let cancelled = false

    async function setWindowIcon() {
      const response = await fetch("/tauri-window-icon.png")
      if (!response.ok || cancelled) return

      const icon = await response.arrayBuffer()
      if (cancelled) return

      const { getCurrentWindow } = await import("@tauri-apps/api/window")
      await getCurrentWindow().setIcon(icon)
    }

    setWindowIcon().catch(() => {
      // The bundled icon is still used when runtime icon updates are unavailable.
    })

    return () => {
      cancelled = true
    }
  }, [])

  return null
}
