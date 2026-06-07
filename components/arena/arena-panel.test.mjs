import assert from "node:assert/strict"
import { existsSync, readFileSync } from "node:fs"
import test from "node:test"

const workbenchSource = readFileSync("components/layout/modeler-workbench.tsx", "utf8")
const apiSource = readFileSync("lib/api.ts", "utf8")

test("arena API exposes markdown card and daily log operations", () => {
  assert.match(apiSource, /listArenaCards\(projectId: string\)/)
  assert.match(apiSource, /createArenaCard\(projectId: string/)
  assert.match(apiSource, /updateArenaCard\(projectId: string/)
  assert.match(apiSource, /appendArenaLog\(projectId: string/)
  assert.match(apiSource, /getArenaIndex\(projectId: string\)/)
})

test("workbench includes arena as an activity", () => {
  assert.match(workbenchSource, /id: "arena"/)
  assert.match(workbenchSource, /ArenaPanel/)
  assert.match(workbenchSource, /activeActivity === "arena"/)
})

test("arena panel supports obsidian-style cards, logs, backlinks, and math markdown", () => {
  assert.ok(existsSync("components/arena/arena-panel.tsx"))
  const panelSource = readFileSync("components/arena/arena-panel.tsx", "utf8")

  assert.match(panelSource, /New Formula/)
  assert.match(panelSource, /New Finding/)
  assert.match(panelSource, /New Assumption/)
  assert.match(panelSource, /New Decision/)
  assert.match(panelSource, /Daily Log/)
  assert.match(panelSource, /Backlinks/)
  assert.match(panelSource, /Unresolved/)
  assert.match(panelSource, /remarkMath/)
  assert.match(panelSource, /rehypeKatex/)
})
