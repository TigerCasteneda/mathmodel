"use client"

import { useCallback, useEffect, useMemo, useState } from "react"
import { listProjectMembers, type ProjectMember } from "@/lib/api"

/**
 * Shared hook for fetching the project's member list.
 *
 * Used by:
 * - the project members admin panel (members + role + capabilities)
 * - the Arena panel (resolving `ArenaCard.created_by` / `last_edited_by`
 *   user_ids to display names for the avatar + byline)
 *
 * Returns both the raw list and a derived `Map<userId, displayName>` for
 * O(1) lookups from any user_id → name. No caching layer — single-screen
 * app, refetch on projectId change is cheap (the `/projects/<id>/members`
 * payload is a few hundred bytes even for big teams).
 *
 * Pattern matches the existing `use-auth`, `use-screen-share`,
 * `use-agent-status` hooks.
 */
export function useProjectMembers(projectId: string | null | undefined): {
  members: ProjectMember[]
  displayNameByUserId: Map<string, string>
  loading: boolean
  refresh: () => Promise<void>
} {
  const [members, setMembers] = useState<ProjectMember[]>([])
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    if (!projectId) {
      setMembers([])
      return
    }
    setLoading(true)
    try {
      const list = await listProjectMembers(projectId)
      setMembers(list)
    } catch {
      // Non-fatal: callers render an "Unknown" fallback when the lookup
      // misses (e.g. legacy cards whose author has left the project).
      setMembers([])
    } finally {
      setLoading(false)
    }
  }, [projectId])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const displayNameByUserId = useMemo(() => {
    const map = new Map<string, string>()
    for (const member of members) {
      // Prefer display_name; fall back to the local part of the email
      // when display_name happens to be empty for any reason.
      const name = member.display_name?.trim() || member.email.split("@")[0] || member.user_id
      map.set(member.user_id, name)
    }
    return map
  }, [members])

  return { members, displayNameByUserId, loading, refresh }
}