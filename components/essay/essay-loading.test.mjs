import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"

const pageSource = readFileSync("app/projects/[id]/essay/page.tsx", "utf8")
const sidebarSource = readFileSync("components/essay/essay-sidebar.tsx", "utf8")
const collabSource = readFileSync("components/essay/use-essay-collab.ts", "utf8")

test("essay collaboration keeps one ydoc per file instead of comparing random guid", () => {
  assert.doesNotMatch(collabSource, /ydocRef\.current\.guid\s*!==\s*fileId/)
  assert.match(collabSource, /fileIdRef/)
  assert.match(collabSource, /seededForFileRef/)
})

test("essay page loads server file content before mounting the editor", () => {
  assert.match(pageSource, /getProjectFileContent/)
  assert.match(pageSource, /await getProjectFileContent\(projectId, fileId\)/)
})

test("essay sidebar preserves local path when navigating between markdown files", () => {
  assert.match(sidebarSource, /params\.set\("file",/)
  assert.match(sidebarSource, /params\.set\("path", file\.path\)/)
  assert.match(sidebarSource, /router\.push\(`\/projects\/\$\{projectId\}\/essay\?\$\{params\.toString\(\)\}`\)/)
})
