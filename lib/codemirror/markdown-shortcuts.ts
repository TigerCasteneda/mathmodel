/**
 * CodeMirror extension: markdown auto-pair shortcuts.
 *
 * Typing an opening markdown delimiter auto-inserts the matching
 * closing delimiter and parks the cursor between them:
 *
 *   "*"  → "*|*"       (italic)
 *   "**" → "**|**"     (bold) — second `*` triggers the close
 *   "_"  → "_|_"
 *   "__" → "__|__"
 *   "`"  → "`|`"       (inline code)
 *
 * Plain `closeBrackets()` doesn't cover these by default and
 * `*`/`**` are deliberately excluded from its built-in pairs because
 * they conflict with bullets and emphasis detection.
 *
 * Implemented as a `Prec.highest` keymap so it wins over
 * `defaultKeymap` but yields to history. Pressing the same delimiter
 * twice in a row is the trigger — single chars still get auto-paired
 * so typing `*hello*` lands as `*hello|*` mid-string.
 */

import { Prec, type Extension } from "@codemirror/state"
import { EditorView, keymap, type KeyBinding } from "@codemirror/view"

interface Pair {
  open: string
  close: string
  /** Set of chars that, when typed immediately before `open`, mean the
   * user is completing a multi-char opener (e.g. typing the second `*`
   * of `**`) and we should trigger the close. */
  doublesWith: string[]
}

const PAIRS: Pair[] = [
  { open: "**", close: "**", doublesWith: ["*"] },
  { open: "*", close: "*", doublesWith: [] },
  { open: "__", close: "__", doublesWith: ["_"] },
  { open: "_", close: "_", doublesWith: [] },
  { open: "`", close: "`", doublesWith: [] },
]

function findPair(ch: string): Pair | undefined {
  // Prefer the multi-char pair when the previous char matches one of
  // its `doublesWith` chars. That way typing `*` then `*` triggers
  // the `**` close, not two separate `*` closes.
  for (const p of PAIRS) {
    if (p.open.length > 1 && p.open[0] === ch) return p
  }
  for (const p of PAIRS) {
    if (p.open === ch) return p
  }
  return undefined
}

function previousChar(view: EditorView): string | null {
  const head = view.state.selection.main.head
  if (head === 0) return null
  const from = Math.max(0, head - 1)
  return view.state.doc.sliceString(from, head)
}

/** Build a keymap where each markdown opener, when typed, inserts the
 * matching closer after the cursor. */
export function markdownShortcuts(): Extension {
  const bindings: KeyBinding[] = []
  for (const pair of PAIRS) {
    for (const ch of pair.open) {
      // Skip the multi-char pair's non-first chars; we trigger only
      // when the user types the first char of the opener (which may
      // be preceded by the matching `doublesWith` char to upgrade).
      if (pair.open.length > 1 && ch !== pair.open[0]) continue
      bindings.push({
        key: ch,
        run(view) {
          // Only auto-pair when the selection is empty (no range
          // replacement semantics to worry about) and the typed char
          // is on the first keystroke of the opener — i.e. preceded
          // by either nothing, whitespace, or a `doublesWith` char.
          const sel = view.state.selection.main
          if (!sel.empty) return false
          const prev = previousChar(view)
          if (prev != null) {
            // Don't auto-pair if we're mid-word or already inside an
            // existing opener of the same kind. Whitespace, opening
            // punctuation, or the doubling char are the only triggers.
            const isWhitespace = /\s/.test(prev)
            const isDoubling = pair.doublesWith.includes(prev)
            if (!isWhitespace && !isDoubling) return false
          }
          // Insert the closing delimiter after the cursor and move
          // the cursor back so it sits between the open and close.
          view.dispatch({
            changes: {
              from: sel.head,
              to: sel.head,
              insert: pair.open + pair.close,
            },
            selection: { anchor: sel.head + pair.open.length },
            scrollIntoView: true,
            userEvent: "input.type",
          })
          return true
        },
      })
    }
  }
  return Prec.high(keymap.of(bindings))
}