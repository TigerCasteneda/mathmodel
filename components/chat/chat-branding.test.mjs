import assert from "node:assert/strict"
import { existsSync, readFileSync, statSync } from "node:fs"
import test from "node:test"

const chatSource = readFileSync("components/chat/chat-panel.tsx", "utf8")
const projectsSource = readFileSync("app/projects/page.tsx", "utf8")
const loginSource = readFileSync("app/login/page.tsx", "utf8")
const sidebarSource = readFileSync("components/dashboard/sidebar.tsx", "utf8")
const layoutSource = readFileSync("app/layout.tsx", "utf8")
const globalsSource = readFileSync("app/globals.css", "utf8")
const cargoSource = readFileSync("src-tauri/Cargo.toml", "utf8")
const capabilitySource = readFileSync("src-tauri/capabilities/default.json", "utf8")

test("chat uses the ModelerMark component with breathing motion", () => {
  // The chat panel renders the new dynamic mark instead of the
  // static Claude spark. Verify by importing ModelerMark with
  // explicit state values, and that the breathing keyframes exist
  // in globals.css.
  assert.match(chatSource, /import.*ModelerMark/)
  // At least one of the chat-panel call sites must use a state prop
  // (we ship idle / thinking / speaking).
  assert.match(chatSource, /state="thinking"/)
  assert.match(chatSource, /state="idle"/)
  assert.match(globalsSource, /@keyframes modeler-mark-breathe/)

  // The state class names are baked into the React component
  // (ModelerMark concatenates `modeler-mark--${state}`), so check
  // them in the component file rather than the chat panel source.
  const modelerMarkSource = readFileSync(
    "components/chat/modeler-mark.tsx",
    "utf8",
  )
  assert.match(modelerMarkSource, /modeler-mark--\$\{state\}/)
  // And confirm the old Claude mark asset is no longer referenced
  // from chat-panel.tsx.
  assert.doesNotMatch(chatSource, /claude-color\.svg/)
  assert.doesNotMatch(chatSource, /cc-claude-breathe/)
  assert.doesNotMatch(chatSource, /OrangeMark/)
})

test("project app mark uses the file box asset without changing favicon metadata", () => {
  assert.match(projectsSource, /\/file-box\.svg/)
  assert.match(sidebarSource, /\/file-box\.svg/)
  assert.doesNotMatch(projectsSource, /Sparkles/)
  assert.doesNotMatch(sidebarSource, /Sparkles/)
})

test("login screen uses the file box mark", () => {
  assert.match(loginSource, /\/file-box\.svg/)
  assert.doesNotMatch(loginSource, /Sparkles/)
})

test("app metadata icon uses the ease curve control points asset", () => {
  assert.match(layoutSource, /\/ease-curve-control-points\.svg/)
  assert.doesNotMatch(layoutSource, /\/icon-light-32x32\.png/)
  assert.doesNotMatch(layoutSource, /\/icon-dark-32x32\.png/)
})

test("tauri desktop icons are generated assets", () => {
  for (const path of [
    "src-tauri/icons/32x32.png",
    "src-tauri/icons/128x128.png",
    "src-tauri/icons/128x128@2x.png",
    "src-tauri/icons/icon.ico",
    "src-tauri/icons/icon.icns",
  ]) {
    assert.ok(statSync(path).size > 100, `${path} should not be a placeholder icon`)
  }
})

test("tauri runtime window icon is explicitly set from the app icon asset", () => {
  assert.match(cargoSource, /features\s*=\s*\[[^\]]*"image-png"/)
  assert.match(capabilitySource, /core:window:allow-set-icon/)

  assert.ok(existsSync("public/tauri-window-icon.png"))
  assert.ok(existsSync("public/ease-curve-control-points-app-icon.svg"))

  const runtimeIconSource = readFileSync("components/tauri-window-icon.tsx", "utf8")
  assert.match(runtimeIconSource, /setIcon/)
  assert.match(runtimeIconSource, /\/tauri-window-icon\.png/)
  assert.match(layoutSource, /<TauriWindowIcon \/>/)
})
