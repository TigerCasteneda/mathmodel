import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"

const searchSource = readFileSync("components/search/search-page.tsx", "utf8")
const workbenchSource = readFileSync("components/layout/modeler-workbench.tsx", "utf8")
const apiSource = readFileSync("lib/tauri-api.ts", "utf8")

test("ai search uses request ids and registers event listeners once", () => {
  assert.match(apiSource, /aiSearch\(query: string, requestId: string\)/)
  assert.match(searchSource, /currentSearchIdRef/)
  assert.match(searchSource, /crypto\.randomUUID\(\)/)
  assert.match(searchSource, /aiSearch\(q,\s*requestId\)/)
  assert.match(searchSource, /event\.request_id !== currentSearchIdRef\.current/)
  assert.doesNotMatch(searchSource, /const cleanup: \(\(\) => void\)\[\] = \[\][\s\S]*?await aiSearch\(q\)/)
})

test("research search ignores stale responses from older requests", () => {
  assert.match(workbenchSource, /researchSearchIdRef/)
  assert.match(workbenchSource, /const requestId = crypto\.randomUUID\(\)/)
  assert.match(workbenchSource, /if \(researchSearchIdRef\.current !== requestId\) return/)
})

test("research panel exposes URL analysis with stale-response protection", () => {
  assert.match(apiSource, /researchAnalyzeUrl\(url: string\)/)
  assert.match(workbenchSource, /researchAnalyzeUrl/)
  assert.match(workbenchSource, /Analyze URL/)
  assert.match(workbenchSource, /urlAnalyzeIdRef/)
  assert.match(workbenchSource, /if \(urlAnalyzeIdRef\.current !== requestId\) return/)
})
