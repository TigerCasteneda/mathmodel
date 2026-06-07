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

test("chat uses the Claude color asset with breathing motion", () => {
  assert.match(chatSource, /\/claude-color\.svg/)
  assert.match(chatSource, /cc-claude-breathe/)
  assert.match(globalsSource, /@keyframes cc-claude-breathe/)
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
