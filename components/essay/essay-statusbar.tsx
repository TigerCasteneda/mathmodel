"use client"

interface EssayStatusBarProps {
  wordCount: number
  sectionIndex: number
  sectionCount: number
  lastSaved: Date | null
}

export function EssayStatusBar({
  wordCount,
  sectionIndex,
  sectionCount,
  lastSaved,
}: EssayStatusBarProps) {
  const savedText = lastSaved
    ? `Saved · ${formatTimeAgo(lastSaved)}`
    : "Unsaved"

  return (
    <div
      className="flex items-center gap-4 border-t border-essay-border bg-essay-bg-alt px-4 text-xs text-essay-text-faint shrink-0 select-none"
      style={{ height: "var(--essay-status-h)" }}
    >
      <span>
        {wordCount.toLocaleString()} words
      </span>
      <span className="text-essay-text-faint opacity-60">|</span>
      <span>
        § {sectionIndex} of {sectionCount}
      </span>
      <span className="flex-1" />
      <span className="flex items-center gap-1.5">
        <span className="inline-block h-1.5 w-1.5 rounded-full bg-green-600" />
        {savedText}
      </span>
    </div>
  )
}

function formatTimeAgo(date: Date): string {
  const seconds = Math.floor((Date.now() - date.getTime()) / 1000)
  if (seconds < 10) return "just now"
  if (seconds < 60) return `${seconds}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ago`
}
