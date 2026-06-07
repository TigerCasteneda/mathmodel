import assert from "node:assert/strict"
import { existsSync, readFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { pathToFileURL } from "node:url"
import test from "node:test"
import ts from "typescript"

async function loadTimelineModule() {
  const sourcePath = "components/chat/chat-timeline.ts"
  assert.equal(existsSync(sourcePath), true, "chat-timeline.ts should exist")
  const source = readFileSync(sourcePath, "utf8")
  const output = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
      jsx: ts.JsxEmit.ReactJSX,
    },
  }).outputText
  const outputPath = join(tmpdir(), `chat-timeline-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  await import("node:fs/promises").then((fs) => fs.writeFile(outputPath, output, "utf8"))
  return import(pathToFileURL(outputPath).href)
}

test("done event without an active assistant does not create an empty assistant turn", async () => {
  const { applyStreamEvent } = await loadTimelineModule()

  const messages = [
    { id: "user-1", role: "user", content: "hello" },
  ]

  const next = applyStreamEvent(messages, {
    conversation_id: "default",
    seq: 1,
    content: "",
    done: true,
  }, false)

  assert.deepEqual(next, messages)
})

test("request failure is not duplicated after an assistant error event", async () => {
  const { appendAssistantError } = await loadTimelineModule()

  const messages = [
    { id: "user-1", role: "user", content: "hello" },
    {
      id: "assistant-1",
      role: "assistant",
      content: "",
      streaming: false,
      timeline: [{ id: "error-1", type: "error", message: "API error" }],
    },
  ]

  const next = appendAssistantError(messages, "Chat request failed.")

  assert.deepEqual(next, messages)
})
