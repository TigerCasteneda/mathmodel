"use client"

/**
 * Compact user avatar — initial-in-circle with a deterministic HSL hue
 * derived from the user_id.
 *
 * Promoted from the local `AvatarBubble` inside `components/arena/arena-chat.tsx`
 * so any feature that needs to display a user (Arena byline, Knowledge
 * Base "sent by", future member-management UI) can reuse the same
 * visual + same color algorithm. The hue math is preserved verbatim so
 * a given `user_id` renders the same color across screens.
 *
 * Size variants:
 *   sm — 20px / 10px text   (sidebar at-a-glance)
 *   md — 32px / xs text     (chat bubbles, editor byline, info tab)
 *
 * When `userId` is null/empty (legacy authorship, deleted member, the
 * server's sentinel for "unknown"), we render a muted gray fallback with
 * a "?" initial so the layout stays stable and the missing identity is
 * visually obvious.
 */

export type UserAvatarSize = "sm" | "md"

const SIZE_CLASSES: Record<UserAvatarSize, string> = {
  sm: "h-5 w-5 text-[10px]",
  md: "h-8 w-8 text-xs",
}

function hueFromUserId(userId: string): number {
  let acc = 0
  for (let i = 0; i < userId.length; i++) {
    acc += userId.charCodeAt(i)
  }
  return acc % 360
}

export interface UserAvatarProps {
  userId: string | null | undefined
  name?: string | null
  size?: UserAvatarSize
  className?: string
}

export function UserAvatar({ userId, name, size = "md", className }: UserAvatarProps) {
  const sizeCls = SIZE_CLASSES[size]
  const resolvedName = name?.trim() || userId || null

  if (!userId) {
    // Unknown / legacy / deleted member — muted gray, "?" initial.
    return (
      <span
        className={`flex shrink-0 items-center justify-center rounded-full bg-[#373737] font-bold text-[#787878] ${sizeCls} ${className ?? ""}`}
        title="Unknown"
        aria-label="Unknown user"
      >
        ?
      </span>
    )
  }

  const initial = ((resolvedName ?? userId).charAt(0) || "?").toUpperCase()
  const hue = hueFromUserId(userId)

  return (
    <span
      className={`flex shrink-0 items-center justify-center rounded-full font-bold text-white ${sizeCls} ${className ?? ""}`}
      style={{ backgroundColor: `hsl(${hue}, 55%, 45%)` }}
      title={resolvedName ?? userId}
      aria-label={resolvedName ?? userId}
    >
      {initial}
    </span>
  )
}