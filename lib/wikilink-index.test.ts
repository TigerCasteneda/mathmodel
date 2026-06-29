/**
 * Unit tests for lib/wikilink-index.ts.
 *
 * Run with: `pnpm exec vitest run lib/wikilink-index.test.ts`
 *   (or whatever runner the project uses — the assertions are plain
 *   Node-compatible so node --test also works.)
 */

import { test } from "node:test"
import assert from "node:assert/strict"
import { WikilinkIndex, normalizeWikilinkTarget } from "./wikilink-index.ts"

test("normalizeWikilinkTarget strips .md and .markdown", () => {
  assert.equal(normalizeWikilinkTarget("note"), "note")
  assert.equal(normalizeWikilinkTarget("note.md"), "note")
  assert.equal(normalizeWikilinkTarget("note.markdown"), "note")
  assert.equal(normalizeWikilinkTarget("note.MD"), "note")
  assert.equal(normalizeWikilinkTarget("  note  "), "note")
})

test("scan records outgoing and inverse incoming", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("See [[other]] and [[third]].", "alpha")
  assert.deepEqual(idx.linksFrom("alpha"), ["other", "third"])
  assert.deepEqual(idx.backlinks("other"), ["alpha"])
  assert.deepEqual(idx.backlinks("third"), ["alpha"])
  assert.deepEqual(idx.backlinks("alpha"), [])
})

test("scan supports the [[name|alias]] form", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("Link to [[target|display text]]", "alpha")
  assert.deepEqual(idx.linksFrom("alpha"), ["target"])
  assert.deepEqual(idx.backlinks("target"), ["alpha"])
})

test("scan ignores self-links", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("This file is [[alpha]] and links to [[beta]].", "alpha")
  assert.deepEqual(idx.linksFrom("alpha"), ["beta"])
  // The self-link doesn't appear in alpha's outgoing set, but more
  // importantly other files linking to alpha should still show up.
  idx.scan("alpha is great", "beta")
  assert.deepEqual(idx.backlinks("alpha"), [])
})

test("scan updates when a target is removed", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("[[a]] [[b]] [[c]]", "alpha")
  assert.deepEqual(idx.backlinks("a"), ["alpha"])
  // Drop the link to "a"
  idx.scan("[[b]] [[c]]", "alpha")
  assert.deepEqual(idx.backlinks("a"), [])
  assert.deepEqual(idx.backlinks("b"), ["alpha"])
})

test("multiple sources pointing to the same target", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("[[topic]]", "alpha")
  idx.scan("related: [[topic]]", "beta")
  idx.scan("see also [[topic]]", "gamma")
  const sources = idx.backlinks("topic")
  assert.equal(sources.length, 3)
  // Sorted alphabetically
  assert.deepEqual(sources, ["alpha", "beta", "gamma"])
})

test("remove clears all references for a source", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("[[x]] [[y]]", "alpha")
  idx.remove("alpha")
  assert.deepEqual(idx.backlinks("x"), [])
  assert.deepEqual(idx.backlinks("y"), [])
  // Remove is idempotent
  idx.remove("alpha")
  assert.equal(idx.size(), 0)
})

test("snapshot round-trips through load()", () => {
  const a = new WikilinkIndex("proj-1")
  a.scan("[[one]] [[two]]", "alpha")
  a.scan("[[one]]", "beta")
  const snap = a.snapshot()

  const b = new WikilinkIndex("proj-1")
  b.load(snap)
  assert.deepEqual(b.linksFrom("alpha"), ["one", "two"])
  assert.deepEqual(b.backlinks("one"), ["alpha", "beta"])
})

test("clear wipes state", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("[[a]]", "alpha")
  idx.clear()
  assert.equal(idx.size(), 0)
  assert.deepEqual(idx.backlinks("a"), [])
})

test("empty text removes all outgoing links for that source", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("[[a]] [[b]]", "alpha")
  assert.equal(idx.size(), 1)
  idx.scan("", "alpha")
  // outgoing for alpha should be gone, so incoming references are also gone
  assert.deepEqual(idx.backlinks("a"), [])
  assert.deepEqual(idx.backlinks("b"), [])
  assert.equal(idx.size(), 0)
})

test("scan with no wikilinks is a no-op", () => {
  const idx = new WikilinkIndex("proj-1")
  idx.scan("just plain text", "alpha")
  assert.equal(idx.size(), 0)
  assert.deepEqual(idx.backlinks("anything"), [])
})

test("scan ignores malformed wikilink brackets", () => {
  const idx = new WikilinkIndex("proj-1")
  // Missing close, missing open, newline inside — all should be ignored
  idx.scan("[[oops [[orphan", "alpha")
  idx.scan("closed only]]", "alpha")
  idx.scan("[[bad\nname]]", "alpha")
  idx.scan("[[good]]", "alpha")
  assert.deepEqual(idx.linksFrom("alpha"), ["good"])
})