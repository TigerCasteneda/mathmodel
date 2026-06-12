// lib/codemirror/awareness.ts

// Color palette for remote cursors
const CURSOR_COLORS = [
  "#f87171", // red
  "#60a5fa", // blue
  "#34d399", // green
  "#fbbf24", // yellow
  "#a78bfa", // purple
  "#f472b6", // pink
  "#38bdf8", // sky
  "#fb923c", // orange
]

// Persist user color per session so same user gets same color on reconnect
let userColorIndex = 0
const userColorMap = new Map<number, string>()

function assignColor(clientId: number): string {
  if (!userColorMap.has(clientId)) {
    userColorMap.set(
      clientId,
      CURSOR_COLORS[userColorIndex % CURSOR_COLORS.length],
    )
    userColorIndex++
  }
  return userColorMap.get(clientId)!
}

// ─── Awareness state helpers ─────────────────────────

export interface AwarenessUserInfo {
  name: string
  color: string
  colorLight: string
}

/**
 * Set local user info in awareness.
 * Call this once after creating the awareness instance.
 */
export function setLocalUserInfo(
  awareness: {
    setLocalStateField: (field: string, value: unknown) => void
    doc?: { clientID: number }
  },
  user: AwarenessUserInfo,
) {
  awareness.setLocalStateField("user", user)
}

/**
 * Extract user info from an awareness state entry.
 */
export function getUserInfo(
  state: Record<string, unknown> | null,
): AwarenessUserInfo | null {
  if (!state) return null
  const user = state.user as AwarenessUserInfo | undefined
  return user ?? null
}
