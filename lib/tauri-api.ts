"use client"

import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface FileTreeItem {
  name: string
  path: string
  type: "file" | "folder"
  language?: string
  children?: FileTreeItem[]
}

export interface PtyOutputEvent {
  type: "pty_output"
  data: string
}

export interface AgentErrorEvent {
  type: "agent_error"
  message: string
}

export interface FileChangeEvent {
  type: "file_change"
  path: string
  content: string
}

export interface FileTreeEvent {
  type: "file_tree"
  tree: FileTreeItem
}

export interface FileContentEvent {
  type: "file_content"
  path: string
  content: string
}

export interface WorkDirEvent {
  type: "work_dir"
  path: string
}

export type AgentEvent =
  | PtyOutputEvent
  | AgentErrorEvent
  | FileChangeEvent
  | FileTreeEvent
  | FileContentEvent
  | WorkDirEvent

export function isTauri(): boolean {
  if (typeof window === "undefined") return false
  // Tauri v2 injects __TAURI_INTERNALS__ even with withGlobalTauri: false
  return "__TAURI_INTERNALS__" in window
    || "__TAURI__" in window
    || "___TAURI_INTERNALS___" in window
}

let cachedPort: number | null = null

export async function getServerPort(): Promise<number> {
  if (cachedPort !== null) return cachedPort
  if (!isTauri()) return 3001
  cachedPort = await invoke<number>("get_server_port")
  return cachedPort
}

// ─── Commands ───────────────────────────────────────

export async function ptySpawn(): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_spawn")
}

export async function ptyWrite(data: string): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_write", { data })
}

export async function ptyResize(cols: number, rows: number): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_resize", { cols, rows })
}

export async function ptyKill(): Promise<void> {
  if (!isTauri()) return
  return invoke("pty_kill")
}

export async function listFiles(): Promise<FileTreeItem> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<FileTreeItem>("list_files")
}

export async function readFile(path: string): Promise<string> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<string>("read_file", { path })
}

export async function createFile(path: string, content: string): Promise<void> {
  if (!isTauri()) return
  return invoke("create_file", { path, content })
}

export async function changeWorkDir(path: string): Promise<FileTreeItem> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<FileTreeItem>("change_work_dir", { path })
}

// ─── Events ─────────────────────────────────────────

function listenEvent<T>(event: string, callback: (payload: T) => void): () => void {
  let cancelled = false
  let unlisten: UnlistenFn | null = null

  listen<T>(event, (e) => {
    if (!cancelled) callback(e.payload)
  }).then((fn) => {
    if (cancelled) fn()
    else unlisten = fn
  })

  return () => {
    cancelled = true
    unlisten?.()
  }
}

export function onPtyOutput(callback: (data: string) => void): () => void {
  return listenEvent<PtyOutputEvent>("pty-output", (e) => callback(e.data))
}

export function onAgentError(callback: (message: string) => void): () => void {
  return listenEvent<AgentErrorEvent>("agent-error", (e) => callback(e.message))
}

export function onFileChange(callback: (path: string, content: string) => void): () => void {
  return listenEvent<FileChangeEvent>("file-change", (e) => callback(e.path, e.content))
}

export function onFileTree(callback: (tree: FileTreeItem) => void): () => void {
  return listenEvent<FileTreeEvent>("file-tree", (e) => callback(e.tree))
}

export function onFileContent(callback: (path: string, content: string) => void): () => void {
  return listenEvent<FileContentEvent>("file-content", (e) => callback(e.path, e.content))
}

export function onWorkDirChanged(callback: (path: string) => void): () => void {
  return listenEvent<WorkDirEvent>("work-dir", (e) => callback(e.path))
}
