"use client"

import { aiChat, onChatStream, isTauri, listFiles, readFile } from "@/lib/tauri-api"
import type { GhostFetcher } from "@/lib/codemirror/ghost-text"

// ─── Context Collection ──────────────────────────────

interface EssayContext {
  essayName: string
  projectMdFiles: Array<{ name: string; content: string }>
}

/**
 * Collect markdown context from project .md files (RAG-lite).
 * Returns up to 5 files, each truncated to 2000 chars.
 */
async function collectProjectContext(
  essayFileName?: string,
): Promise<EssayContext> {
  const result: EssayContext = {
    essayName: essayFileName ?? "essay.md",
    projectMdFiles: [],
  }

  if (!isTauri()) return result

  try {
    const tree = await listFiles()
    const mdFiles = findMdFiles(tree)
      .filter((f) => f.path !== essayFileName)
      .slice(0, 5)

    for (const file of mdFiles) {
      try {
        const content = await readFile(file.path)
        result.projectMdFiles.push({
          name: file.name,
          content: content.slice(0, 2000),
        })
      } catch {
        // skip unreadable files
      }
    }
  } catch {
    // ignore
  }

  return result
}

function findMdFiles(
  tree: { name: string; path: string; type: string; children?: unknown[] },
): Array<{ name: string; path: string }> {
  const results: Array<{ name: string; path: string }> = []
  if (tree.type === "file" && tree.name.endsWith(".md")) {
    results.push({ name: tree.name, path: tree.path })
  }
  if (tree.children) {
    for (const child of tree.children) {
      results.push(...findMdFiles(child as any))
    }
  }
  return results
}

// ─── Prompt Builder ──────────────────────────────────

function buildGhostPrompt(
  prefix: string,
  suffix: string,
  context: EssayContext,
): string {
  let prompt = ""

  prompt += "Continue writing the following essay naturally. "
  prompt += "Match the style, tone, and detail level. "
  prompt += "Do NOT repeat existing content. "
  prompt += "Write in the same language as the text below.\n\n"

  // Reference materials
  if (context.projectMdFiles.length > 0) {
    prompt += "Reference materials from the project:\n\n"
    for (const file of context.projectMdFiles) {
      prompt += `### ${file.name}\n${file.content}\n\n`
    }
    prompt += "---\n\n"
  }

  // Context
  prompt += `Current text before cursor:\n${prefix}\n\n`
  if (suffix.trim()) {
    prompt += `Current text after cursor:\n${suffix}\n\n`
  }
  prompt += "Continue writing:"

  return prompt
}

// ─── Ghost Fetcher ───────────────────────────────────

const GHOST_CONVERSATION_PREFIX = "essay-ghost-"

/**
 * Creates a GhostFetcher bound to a specific essay file.
 * Call once per file; returns the fetcher that the ghost plugin uses.
 */
export function createGhostFetcher(
  fileId: string,
  essayFileName?: string,
  serverBase?: string,
): GhostFetcher {
  const conversationId = `${GHOST_CONVERSATION_PREFIX}${fileId}`
  // Cache context per file (loaded once)
  let contextPromise: Promise<EssayContext> | null = null

  return (prefix, suffix, signal, onToken, onDone) => {
    if (!isTauri()) {
      onDone()
      return
    }

    // Load context lazily
    if (!contextPromise) {
      contextPromise = collectProjectContext(essayFileName)
    }

    contextPromise.then((context) => {
      if (signal.aborted) return

      const prompt = buildGhostPrompt(prefix, suffix, context)

      // Listen for stream events — filter by conversationId
      const stopListening = onChatStream((event) => {
        if (signal.aborted) {
          stopListening()
          return
        }
        if (event.conversation_id !== conversationId) return

        if (event.content) {
          onToken(event.content)
        }
        if (event.done) {
          stopListening()
          onDone()
        }
      })

      // Send the request
      aiChat(prompt, conversationId, {
        workspaceMode: "host",
        permissionMode: "auto",
        serverBase: serverBase,
      }).catch(() => {
        stopListening()
        onDone()
      })

      // Abort handling
      const onAbort = () => {
        stopListening()
        onDone()
      }
      signal.addEventListener("abort", onAbort, { once: true })
    })
  }
}
