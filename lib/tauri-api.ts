"use client"

import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface FileTreeItem {
  id?: string
  name: string
  path: string
  type: "file" | "folder"
  zone?: string
  updated_at?: number
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
  | AgentErrorEvent
  | FileChangeEvent
  | FileTreeEvent
  | FileContentEvent
  | WorkDirEvent

export interface AiConfig {
  api_key?: string | null
  base_url: string
  model: string
  firecrawl_api_key?: string | null
  context7_api_key?: string | null
  tavily_api_key?: string | null
  searxng_url: string
}

export interface AiConfigStatus {
  configured: boolean
  base_url: string
  model: string
  firecrawl_configured: boolean
  context7_configured: boolean
  tavily_configured: boolean
  searxng_url: string
}

export interface ChatStreamEvent {
  conversation_id: string
  seq?: number
  content: string
  done: boolean
}

export interface ChatErrorEvent {
  conversation_id: string
  message: string
}

export type AiPermissionMode = "default" | "accept_edit" | "auto" | "bypass"

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
  return Promise.resolve()
}

export async function ptyWrite(data: string): Promise<void> {
  void data
  return Promise.resolve()
}

export async function ptyResize(cols: number, rows: number): Promise<void> {
  void cols
  void rows
  return Promise.resolve()
}

export async function ptyKill(): Promise<void> {
  return Promise.resolve()
}

export async function listFiles(): Promise<FileTreeItem> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<FileTreeItem>("list_files")
}

export async function readFile(path: string): Promise<string> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<string>("read_file", { path })
}

export async function readFileBase64(path: string): Promise<string> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<string>("read_file_base64", { path })
}

export async function writeFile(path: string, content: string): Promise<void> {
  if (!isTauri()) return
  return invoke("write_file", { path, content })
}

export async function createFile(path: string, content: string): Promise<void> {
  if (!isTauri()) return
  return invoke("create_file", { path, content })
}

export async function changeWorkDir(path: string): Promise<FileTreeItem> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<FileTreeItem>("change_work_dir", { path })
}

export async function setAiConfig(config: AiConfig): Promise<void> {
  if (!isTauri()) return
  return invoke("set_ai_config", { config })
}

export async function getAiConfigStatus(): Promise<AiConfigStatus> {
  if (!isTauri()) {
    return {
      configured: false,
      base_url: "https://api.deepseek.com",
      model: "deepseek-v4-pro",
      firecrawl_configured: false,
      context7_configured: false,
      tavily_configured: false,
      searxng_url: "http://localhost:8080",
    }
  }
  return invoke<AiConfigStatus>("get_ai_config_status")
}

export async function setAiModel(model: string): Promise<AiConfigStatus | null> {
  if (!isTauri()) return null
  return invoke<AiConfigStatus>("set_ai_model", { model })
}

export async function openFolder(): Promise<string | null> {
  if (!isTauri()) return null
  return invoke<string | null>("open_folder")
}

export interface AiChatOptions {
  workspaceMode?: "host" | "guest"
  permissionMode?: AiPermissionMode
  projectId?: string
  authToken?: string | null
  serverBase?: string | null
  capabilities?: string[]
}

export async function aiChat(
  message: string,
  conversationId = "default",
  options: AiChatOptions = {},
): Promise<void> {
  if (!isTauri()) return
  const serverBase = options.serverBase ?? `http://127.0.0.1:${await getServerPort()}`
  return invoke("ai_chat", {
    message,
    conversationId,
    workspaceMode: options.workspaceMode ?? "host",
    permissionMode: options.permissionMode ?? "default",
    projectId: options.projectId ?? null,
    authToken: options.authToken ?? null,
    serverBase,
    capabilities: options.capabilities ?? null,
  })
}

// ─── Events ─────────────────────────────────────────

export type ResearchSearchKind = "auto" | "web" | "paper" | "dataset" | "code" | "docs"

export interface NativeResearchSearchItem {
  title: string
  url: string
  content: string
  provider: string
  source: string
  category: string
  authors?: string | null
  publish_year?: number | null
  keywords?: string | null
  relevance_score: number
  raw_json: Record<string, unknown>
  planned_kind?: ResearchSearchKind | null
  planned_query?: string | null
  reason?: string | null
  rank?: number | null
}

export interface NativeResearchSearchResponse {
  query: string
  kind: ResearchSearchKind
  results: NativeResearchSearchItem[]
  warning?: string | null
}

export interface NativeResearchSaveRequest {
  project_id: string
  results: NativeResearchSearchItem[]
  kind: ResearchSearchKind
  auth_token?: string | null
  server_base?: string | null
}

export interface NativeResearchSaveResponse {
  saved: number
  items: unknown[]
  files_created: number
}

export async function researchSearchNative(
  query: string,
  kind: ResearchSearchKind,
  maxResults = 8,
): Promise<NativeResearchSearchResponse> {
  if (!isTauri()) return { query, kind, results: [] }
  return invoke<NativeResearchSearchResponse>("research_search_native", {
    query,
    kind,
    maxResults,
  })
}

export async function researchExtractAndSave(
  request: NativeResearchSaveRequest,
): Promise<NativeResearchSaveResponse> {
  if (!isTauri()) return { saved: 0, items: [], files_created: 0 }
  const serverBase = request.server_base ?? `http://127.0.0.1:${await getServerPort()}`
  return invoke<NativeResearchSaveResponse>("research_extract_and_save", {
    request: {
      ...request,
      server_base: serverBase,
    },
  })
}

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
  void callback
  return () => {}
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

export interface ChatToolCallEvent {
  conversation_id: string
  name: string
  arguments: Record<string, unknown>
  output: string
  status: string
}

export interface ChatBackgroundTaskEvent {
  conversation_id: string
  task_id: string
  task_type: string
  prompt: string
  status: "running" | "completed" | "error"
  result: string
}

export interface PermissionRequestEvent {
  request_id: string
  conversation_id: string
  tool_name: string
  arguments: Record<string, unknown>
  reason: string
  mode: AiPermissionMode
  content?: string | null
  expires_at_ms: number
}

export function onChatStream(callback: (event: ChatStreamEvent) => void): () => void {
  return listenEvent<ChatStreamEvent>("chat:stream", callback)
}

export function onChatToolCall(callback: (event: ChatToolCallEvent) => void): () => void {
  return listenEvent<ChatToolCallEvent>("chat:tool_call", callback)
}

export function onChatError(callback: (event: ChatErrorEvent) => void): () => void {
  return listenEvent<ChatErrorEvent>("chat:error", callback)
}

export function onChatBackgroundTask(callback: (event: ChatBackgroundTaskEvent) => void): () => void {
  return listenEvent<ChatBackgroundTaskEvent>("chat:background_task", callback)
}

export function onPermissionRequest(callback: (event: PermissionRequestEvent) => void): () => void {
  return listenEvent<PermissionRequestEvent>("chat:permission_request", callback)
}

// ─── Search ────────────────────────────────────────

export interface SearchResultItem {
  title: string
  url: string
  content: string
  score: number
}

export interface SearchResultsEvent {
  query: string
  results: SearchResultItem[]
}

export interface SearchStreamEvent {
  seq: number
  content: string
  done: boolean
}

export interface SearchQuestionsEvent {
  questions: string[]
}

export interface SearchErrorEvent {
  message: string
}

export function onSearchResults(callback: (event: SearchResultsEvent) => void): () => void {
  return listenEvent<SearchResultsEvent>("search:results", callback)
}

export function onSearchStream(callback: (event: SearchStreamEvent) => void): () => void {
  return listenEvent<SearchStreamEvent>("search:stream", callback)
}

export function onSearchQuestions(callback: (event: SearchQuestionsEvent) => void): () => void {
  return listenEvent<SearchQuestionsEvent>("search:questions", callback)
}

export function onSearchError(callback: (event: SearchErrorEvent) => void): () => void {
  return listenEvent<SearchErrorEvent>("search:error", callback)
}

export async function aiSearch(query: string): Promise<void> {
  if (!isTauri()) return
  return invoke("ai_search", { query })
}

export async function resolvePermissionRequest(requestId: string, allow: boolean): Promise<void> {
  if (!isTauri()) return
  return invoke("resolve_permission_request", { requestId, allow })
}

// ─── Sessions ────────────────────────────────────────

export interface SessionInfo {
  id: string
  name: string
  created_at: number
  message_count: number
}

export interface Session {
  id: string
  name: string
  created_at: number
  updated_at: number
  messages: SessionMessage[]
}

export interface SessionToolCallFunction {
  name: string
  arguments: string
}

export interface SessionToolCall {
  id: string
  type: string
  function: SessionToolCallFunction
}

export interface SessionMessage {
  role: string
  content?: string | null
  timestamp: number
  tool_calls?: SessionToolCall[] | null
  tool_call_id?: string | null
}

export async function listSessions(): Promise<SessionInfo[]> {
  if (!isTauri()) return []
  return invoke<SessionInfo[]>("list_sessions")
}

export async function loadSession(conversationId?: string): Promise<Session> {
  if (!isTauri()) return { id: "default", name: "New Chat", created_at: 0, updated_at: 0, messages: [] }
  return invoke<Session>("load_session", { conversationId: conversationId || null })
}

export async function deleteSession(conversationId: string): Promise<void> {
  if (!isTauri()) return
  return invoke("delete_session", { conversationId })
}
