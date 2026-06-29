/**
 * Wikilink index — `[[note]]` reference graph for the essay editor.
 *
 * Tracks which notes mention which other notes so the Backlinks panel
 * (see `components/essay/essay-backlinks-panel.tsx`) can answer
 * "what notes link to the current one?" without re-scanning every
 * open note's content.
 *
 * The index is **client-side only** (no server roundtrip) and
 * persisted to localStorage per-project. It's debounced on the hot
 * path: the editor's `updateListener` calls `scan(text, source)`
 * with the current note's text, and we diff against the previous
 * outgoing set so we only update `incoming` for changed targets.
 *
 * Targets are normalized to **basenames without `.md`** — that's the
 * canonical form the autocomplete popup and click handlers use.
 */

const STORAGE_PREFIX = "wikilink-index:"

/** Match `[[name]]` or `[[name|display]]`. The inner name can't
 * contain `[`, `]`, or newline. Captures (1) the raw inner name. */
const WIKILINK_RE = /\[\[([^\[\]\n|]+?)(?:\|[^\]\n]+?)?\]\]/g

/** Strip a trailing `.md` / `.markdown` so links and file lookups agree. */
export function normalizeWikilinkTarget(raw: string): string {
  return raw.trim().replace(/\.(md|markdown)$/i, "")
}

export interface WikilinkIndexSnapshot {
  /** source basename → sorted list of target basenames it links to */
  outgoing: Record<string, string[]>
  /** target basename → sorted list of source basenames linking to it */
  incoming: Record<string, string[]>
}

export class WikilinkIndex {
  private outgoing = new Map<string, Set<string>>()
  private incoming = new Map<string, Set<string>>()
  private projectId: string
  private storageKey: string

  constructor(projectId: string) {
    this.projectId = projectId
    this.storageKey = `${STORAGE_PREFIX}${projectId}`
  }

  /**
   * Scan a note's text and update the outgoing/incoming maps for that source.
   * Replaces any previous outgoing entries for `source`.
   */
  scan(text: string, source: string): void {
    const previous = this.outgoing.get(source) ?? new Set<string>()
    const next = new Set<string>()
    for (const match of text.matchAll(WIKILINK_RE)) {
      const target = normalizeWikilinkTarget(match[1])
      if (target && target !== source) next.add(target)
    }

    if (setsEqual(previous, next)) return

    // Remove `source` from `incoming` for targets it no longer links to.
    for (const oldTarget of previous) {
      if (!next.has(oldTarget)) {
        removeFromSet(this.incoming, oldTarget, source)
      }
    }
    // Add `source` to `incoming` for new targets.
    for (const newTarget of next) {
      if (!previous.has(newTarget)) {
        addToSet(this.incoming, newTarget, source)
      }
    }
    if (next.size === 0) {
      this.outgoing.delete(source)
    } else {
      this.outgoing.set(source, next)
    }
  }

  /** Drop all references to a source note (called when a tab is closed). */
  remove(source: string): void {
    const targets = this.outgoing.get(source)
    if (!targets) return
    for (const target of targets) {
      removeFromSet(this.incoming, target, source)
    }
    this.outgoing.delete(source)
  }

  /** Sorted list of source basenames that link to `target`. */
  backlinks(target: string): string[] {
    const sources = this.incoming.get(target)
    if (!sources) return []
    return [...sources].sort()
  }

  /** All targets a source links to (sorted, deduplicated). */
  linksFrom(source: string): string[] {
    const targets = this.outgoing.get(source)
    if (!targets) return []
    return [...targets].sort()
  }

  /** Number of source notes the index knows about. */
  size(): number {
    return this.outgoing.size
  }

  /** Snapshot for debugging / persistence. */
  snapshot(): WikilinkIndexSnapshot {
    const out: WikilinkIndexSnapshot = { outgoing: {}, incoming: {} }
    for (const [k, v] of this.outgoing) out.outgoing[k] = [...v].sort()
    for (const [k, v] of this.incoming) out.incoming[k] = [...v].sort()
    return out
  }

  /** Replace state from a persisted snapshot. */
  load(snapshot: WikilinkIndexSnapshot): void {
    this.outgoing.clear()
    this.incoming.clear()
    for (const [k, vs] of Object.entries(snapshot.outgoing ?? {})) {
      this.outgoing.set(k, new Set(vs))
    }
    for (const [k, vs] of Object.entries(snapshot.incoming ?? {})) {
      this.incoming.set(k, new Set(vs))
    }
  }

  /** Persist current snapshot to localStorage. No-op outside browser. */
  persist(): void {
    if (typeof window === "undefined") return
    try {
      window.localStorage.setItem(
        this.storageKey,
        JSON.stringify(this.snapshot()),
      )
    } catch {
      // QuotaExceeded / private mode — silently ignore. The in-memory
      // index still works for the current session.
    }
  }

  /** Load from localStorage on construction. Safe to call repeatedly. */
  hydrate(): void {
    if (typeof window === "undefined") return
    const raw = window.localStorage.getItem(this.storageKey)
    if (!raw) return
    try {
      this.load(JSON.parse(raw) as WikilinkIndexSnapshot)
    } catch {
      // Corrupt snapshot — discard; rebuild as we scan.
      window.localStorage.removeItem(this.storageKey)
    }
  }

  /** For tests: wipe state. */
  clear(): void {
    this.outgoing.clear()
    this.incoming.clear()
  }
}

// ── internal helpers ─────────────────────────────────────────────────────

function addToSet(map: Map<string, Set<string>>, key: string, value: string): void {
  let bucket = map.get(key)
  if (!bucket) {
    bucket = new Set()
    map.set(key, bucket)
  }
  bucket.add(value)
}

function removeFromSet(map: Map<string, Set<string>>, key: string, value: string): void {
  const bucket = map.get(key)
  if (!bucket) return
  bucket.delete(value)
  if (bucket.size === 0) map.delete(key)
}

function setsEqual(a: Set<string>, b: Set<string>): boolean {
  if (a.size !== b.size) return false
  for (const v of a) if (!b.has(v)) return false
  return true
}