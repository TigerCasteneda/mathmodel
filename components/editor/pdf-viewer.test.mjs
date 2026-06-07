import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"

const viewerSource = readFileSync("components/editor/pdf-viewer.tsx", "utf8")
const rendererSource = readFileSync("components/editor/pdf-document-renderer.tsx", "utf8")
const workbenchSource = readFileSync("components/layout/modeler-workbench.tsx", "utf8")
const commandsSource = readFileSync("src-tauri/src/agent/commands.rs", "utf8")

test("pdf viewer uses a client-only react-pdf renderer instead of browser iframe plugin", () => {
  assert.match(viewerSource, /dynamic\(/)
  assert.match(viewerSource, /ssr:\s*false/)
  assert.match(rendererSource, /from "react-pdf"/)
  assert.match(rendererSource, /\bDocument\b/)
  assert.match(rendererSource, /\bPage\b/)
  assert.doesNotMatch(viewerSource, /<iframe\b/)
})

test("pdf renderer configures a pdfjs worker for WebView rendering", () => {
  assert.match(rendererSource, /pdfjs\.GlobalWorkerOptions\.workerSrc/)
  assert.match(rendererSource, /pdf\.worker/)
})

test("workbench routes local images to a binary image preview instead of the text editor", () => {
  assert.match(workbenchSource, /png/)
  assert.match(workbenchSource, /jpe?g/)
  assert.match(workbenchSource, /ImageViewer/)
  assert.match(workbenchSource, /active\.language === "image"/)
  assert.match(workbenchSource, /lang === "pdf" \|\| lang === "image"/)
})

test("binary preview reads workspace files even when passed an absolute in-workspace path", () => {
  assert.match(commandsSource, /relative_or_workspace_path/)
  assert.doesNotMatch(commandsSource, /if rel\.is_absolute\(\) \{\s*return Err\("absolute path rejected"\.into\(\)\);\s*\}/)
})
