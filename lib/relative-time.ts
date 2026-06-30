/**
 * Format a Unix-millisecond timestamp as a relative phrase
 * ("just now", "5 minutes ago", "yesterday", "3 days ago").
 *
 * Uses `Intl.RelativeTimeFormat` with `numeric: "auto"` so thresholds
 * round to natural language ("yesterday" / "tomorrow") instead of
 * "1 day ago" / "in 1 day" once we cross 24h.
 *
 * The `< 60s → "just now"` short-circuit avoids `Intl`'s tendency to
 * produce "in 0 minutes" / "0 minutes ago" garbage right after an
 * event. The `now` parameter is overridable for unit tests.
 *
 * Stable across locales that ship `Intl.RelativeTimeFormat` (every
 * evergreen browser + Node ≥ 16). No external dep.
 */

const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" })

const MINUTE_MS = 60_000
const HOUR_MS = 60 * MINUTE_MS
const DAY_MS = 24 * HOUR_MS
const WEEK_MS = 7 * DAY_MS
const MONTH_MS = 30 * DAY_MS
const YEAR_MS = 365 * DAY_MS

export function formatRelativeTime(ts: number, now: number = Date.now()): string {
  const diffMs = ts - now
  const absMs = Math.abs(diffMs)

  if (absMs < MINUTE_MS) return "just now"
  if (absMs < HOUR_MS) return rtf.format(Math.round(diffMs / MINUTE_MS), "minute")
  if (absMs < DAY_MS) return rtf.format(Math.round(diffMs / HOUR_MS), "hour")
  if (absMs < WEEK_MS) return rtf.format(Math.round(diffMs / DAY_MS), "day")
  if (absMs < MONTH_MS) return rtf.format(Math.round(diffMs / WEEK_MS), "week")
  if (absMs < YEAR_MS) return rtf.format(Math.round(diffMs / MONTH_MS), "month")
  return rtf.format(Math.round(diffMs / YEAR_MS), "year")
}