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

export interface FileBinaryChangeEvent {
  type: "file_binary_change"
  path: string
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
  | FileBinaryChangeEvent
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
  sidecar_enabled?: boolean
  sidecar_python_path?: string | null
}

export interface AiConfigStatus {
  configured: boolean
  base_url: string
  model: string
  firecrawl_configured: boolean
  context7_configured: boolean
  tavily_configured: boolean
  searxng_url: string
  sidecar_enabled: boolean
}

export interface ChatStreamEvent {
  conversation_id: string
  seq?: number
  content: string
  done: boolean
}

export interface ChatThinkingEvent {
  conversation_id: string
  content: string
}

export interface ChatTokenUsageEvent {
  conversation_id: string
  prompt_tokens: number
  completion_tokens: number
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

export async function getWorkDir(): Promise<string | null> {
  if (!isTauri()) return null
  return invoke<string | null>("get_work_dir")
}

/**
 * Open a URL in the system default browser. In Tauri, this routes through
 * tauri-plugin-shell so the OS picks the handler (browser, pdf viewer, etc.).
 * In a plain browser context, falls back to a noopener new-tab open.
 *
 * The caller should still keep an `href` on the link so right-click →
 * "Open in new tab" still works in dev; this helper exists for left-click
 * and explicit "open" actions.
 */
export async function openUrl(url: string): Promise<void> {
  if (!url) return
  if (isTauri()) {
    const { open } = await import("@tauri-apps/plugin-shell")
    await open(url)
    return
  }
  if (typeof window !== "undefined") {
    window.open(url, "_blank", "noopener,noreferrer")
  }
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
      sidecar_enabled: true,
    }
  }
  return invoke<AiConfigStatus>("get_ai_config_status")
}

/** Whether the research sidecar process is currently running and healthy. */
export async function getSidecarStatus(): Promise<boolean> {
  if (!isTauri()) return false
  try {
    return await invoke<boolean>("get_sidecar_status")
  } catch {
    return false
  }
}

export async function setAiModel(model: string): Promise<AiConfigStatus | null> {
  if (!isTauri()) return null
  return invoke<AiConfigStatus>("set_ai_model", { model })
}

export async function openFolder(): Promise<string | null> {
  if (!isTauri()) return null
  return invoke<string | null>("open_folder")
}

export interface LatexCompileResult {
  success: boolean
  pdf_path: string | null
  log: string
}

// Compile a workspace .tex file to PDF via latexmk (Host Local mode only).
export async function compileLatex(path: string): Promise<LatexCompileResult> {
  if (!isTauri()) throw new Error("Not running in Tauri")
  return invoke<LatexCompileResult>("compile_latex", { path })
}

export interface AiChatOptions {
  workspaceMode?: "host" | "guest"
  permissionMode?: AiPermissionMode
  projectId?: string
  authToken?: string | null
  serverBase?: string | null
  capabilities?: string[]
  /** Authenticated user id; scopes persisted chat sessions on disk. */
  userId?: string | null
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
    userId: options.userId ?? null,
  })
}

export async function stopGeneration(conversationId = "default"): Promise<void> {
  if (!isTauri()) return
  return invoke("stop_generation", { conversationId })
}

// ─── Events ─────────────────────────────────────────

export type ResearchSearchKind = "auto" | "web" | "paper" | "dataset" | "code" | "docs"

export type ResearchScraper = "scrapling" | "firecrawl" | "tavily"

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
  /**
   * Workspace mode from the frontend. `"host"` enables a one-way mirror of
   * the server-saved items into `host_folder/references/`. Anything else
   * (including `undefined`) keeps the legacy server-only behavior.
   */
  workspace_mode?: "host" | "guest" | null
  /** Absolute path to the host workspace root; required when host-mode mirroring. */
  host_folder?: string | null
}

export interface NativeResearchFileMirror {
  cloud_file_id: string
  file_name: string
  body_md: string
  bib_file_name?: string | null
  body_bib?: string | null
  title: string
  url: string
}

export interface NativeResearchLocalMirror {
  attempted: number
  created: number
  skipped: number
  errors: Array<{ file_name: string; error: string }>
}

export interface NativeResearchSaveResponse {
  saved: number
  items: unknown[]
  files_created: number
  warnings?: string[] | null
  /** Per-item mirror metadata returned by the server for host-mode mirroring. */
  mirrors?: NativeResearchFileMirror[]
  /** Summary of what the host agent wrote to local disk; absent in guest mode. */
  local_mirror?: NativeResearchLocalMirror
}

export async function researchSearchNative(
  query: string,
  kind: ResearchSearchKind,
  maxResults = 16,
  scraper: ResearchScraper = "firecrawl",
): Promise<NativeResearchSearchResponse> {
  if (!isTauri()) return { query, kind, results: [] }
  return invoke<NativeResearchSearchResponse>("research_search_native", {
    query,
    kind,
    maxResults,
    scraper,
  })
}

export async function researchAnalyzeUrl(url: string): Promise<NativeResearchSearchItem> {
  if (!isTauri()) {
    return {
      title: url,
      url,
      content: "",
      provider: "user_url",
      source: "user_url",
      category: "literature",
      relevance_score: 0,
      raw_json: {},
    }
  }
  return invoke<NativeResearchSearchItem>("research_analyze_url", { url })
}

export async function researchExtractAndSave(
  request: NativeResearchSaveRequest,
): Promise<NativeResearchSaveResponse> {
  if (!isTauri()) return { saved: 0, items: [], files_created: 0, warnings: [] }
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

export function onFileBinaryChange(callback: (path: string) => void): () => void {
  return listenEvent<FileBinaryChangeEvent>("file-binary-change", (e) => callback(e.path))
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
  id?: string
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

export function onChatThinking(callback: (event: ChatThinkingEvent) => void): () => void {
  return listenEvent<ChatThinkingEvent>("chat:thinking", callback)
}

export function onChatTokenUsage(callback: (event: ChatTokenUsageEvent) => void): () => void {
  return listenEvent<ChatTokenUsageEvent>("chat:token_usage", callback)
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
  request_id: string
  query: string
  results: SearchResultItem[]
}

export interface SearchStreamEvent {
  request_id: string
  seq: number
  content: string
  done: boolean
}

export interface SearchQuestionsEvent {
  request_id: string
  questions: string[]
}

export interface SearchErrorEvent {
  request_id: string
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

export async function aiSearch(query: string, requestId: string): Promise<void> {
  if (!isTauri()) return
  return invoke("ai_search", { query, requestId })
}

// ─── Agentic research ──

export interface AgentThinkingEvent {
  request_id: string
  content: string
}

export interface AgentToolEvent {
  request_id: string
  id: string
  name: string
  arguments: Record<string, unknown>
  status: "running" | "success" | "error"
  summary: string
}

export interface AgentSource {
  citation: number
  title: string
  url: string
  content: string
  provider: string
  category: string
  structured_data?: Record<string, unknown> | null
}

export interface AgentResultsEvent {
  request_id: string
  results: AgentSource[]
}

export interface AgentSourceUpdateEvent {
  request_id: string
  citation: number
  structured_data: Record<string, unknown> | null
}

export interface AgentStreamEvent {
  request_id: string
  seq: number
  content: string
  done: boolean
}

export interface AgentErrorEvent {
  request_id: string
  message: string
}

export interface AgentDoneEvent {
  request_id: string
}

export function onResearchAgentThinking(callback: (event: AgentThinkingEvent) => void): () => void {
  return listenEvent<AgentThinkingEvent>("research_agent:thinking", callback)
}

export function onResearchAgentTool(callback: (event: AgentToolEvent) => void): () => void {
  return listenEvent<AgentToolEvent>("research_agent:tool", callback)
}

export function onResearchAgentResults(callback: (event: AgentResultsEvent) => void): () => void {
  return listenEvent<AgentResultsEvent>("research_agent:results", callback)
}

export function onResearchAgentSourceUpdate(
  callback: (event: AgentSourceUpdateEvent) => void,
): () => void {
  return listenEvent<AgentSourceUpdateEvent>("research_agent:source_update", callback)
}

export function onResearchAgentStream(callback: (event: AgentStreamEvent) => void): () => void {
  return listenEvent<AgentStreamEvent>("research_agent:stream", callback)
}

export function onResearchAgentError(callback: (event: AgentErrorEvent) => void): () => void {
  return listenEvent<AgentErrorEvent>("research_agent:error", callback)
}

export function onResearchAgentDone(callback: (event: AgentDoneEvent) => void): () => void {
  return listenEvent<AgentDoneEvent>("research_agent:done", callback)
}

export async function researchAgentRun(
  query: string,
  requestId: string,
  conversationId: string,
  scraper: ResearchScraper = "firecrawl",
  userId: string | null = null,
): Promise<void> {
  if (!isTauri()) return
  return invoke("research_agent_run", {
    query,
    requestId,
    conversationId,
    scraper,
    userId,
  })
}

export async function resolvePermissionRequest(
  userId: string,
  requestId: string,
  allow: boolean,
): Promise<void> {
  if (!isTauri()) return
  return invoke("resolve_permission_request", { userId, requestId, allow })
}

// ─── Questions ──

export interface QuestionOption {
  label: string
  description: string
}

export interface QuestionItem {
  question: string
  header: string
  options: QuestionOption[]
  multiSelect: boolean
}

export interface QuestionEvent {
  request_id: string
  conversation_id: string
  questions: QuestionItem[]
  expires_at_ms: number
}

export function onQuestion(callback: (event: QuestionEvent) => void): () => void {
  if (!isTauri()) return () => {}
  const unlisten = listen<QuestionEvent>("chat:question", (event) => callback(event.payload))
  return () => { unlisten.then((fn) => fn()) }
}

export async function resolveQuestion(requestId: string, answers: Record<string, any>): Promise<void> {
  if (!isTauri()) return
  return invoke("resolve_question", { requestId, answers: JSON.stringify(answers) })
}

// ─── Agent events ──

export interface AgentSessionEvent {
  session_id: string
  agent_type: string
  status: string
  prompt: string
  result?: string | null
}

export function onAgentStart(callback: (event: AgentSessionEvent) => void): UnlistenFn {
  const p = listen<AgentSessionEvent>("chat:agent_start", (e) => callback(e.payload))
  return () => { p.then((fn) => fn()) }
}

export function onAgentComplete(callback: (event: { session_id: string; result: string }) => void): UnlistenFn {
  const p = listen<{ session_id: string; result: string }>("chat:agent_complete", (e) => callback(e.payload))
  return () => { p.then((fn) => fn()) }
}

// ─── Tasks ──

export async function listTasks(): Promise<any[]> {
  if (!isTauri()) return []
  return invoke<any[]>("list_tasks", { conversationId: "default" })
}

// ─── Sessions ────────────────────────────────────────

export interface SessionInfo {
  id: string
  name: string
  created_at: number
  message_count: number
  status: string
}

export interface Session {
  id: string
  name: string
  created_at: number
  updated_at: number
  messages: SessionMessage[]
  status: string
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

export async function listSessions(userId: string): Promise<SessionInfo[]> {
  if (!isTauri()) return []
  return invoke<SessionInfo[]>("list_sessions", { userId })
}

export async function loadSession(
  userId: string,
  conversationId?: string,
): Promise<Session> {
  if (!isTauri()) return { id: "default", name: "New Chat", created_at: 0, updated_at: 0, messages: [], status: "active" }
  return invoke<Session>("load_session", { userId, conversationId: conversationId || null })
}

export async function deleteSession(userId: string, conversationId: string): Promise<void> {
  if (!isTauri()) return
  return invoke("delete_session", { userId, conversationId })
}

export async function renameSession(userId: string, conversationId: string, newName: string): Promise<void> {
  if (!isTauri()) return
  return invoke("rename_session", { userId, conversationId, newName })
}

export async function archiveSession(userId: string, conversationId: string): Promise<void> {
  if (!isTauri()) return
  return invoke("archive_session", { userId, conversationId })
}

export async function unarchiveSession(userId: string, conversationId: string): Promise<void> {
  if (!isTauri()) return
  return invoke("unarchive_session", { userId, conversationId })
}

export async function searchSessions(userId: string, query: string): Promise<SessionInfo[]> {
  if (!isTauri()) return []
  return invoke<SessionInfo[]>("search_sessions", { userId, query })
}

export async function exportSession(
  userId: string,
  conversationId: string,
): Promise<SessionMessage[]> {
  if (!isTauri()) return []
  return invoke<SessionMessage[]>("export_session", { userId, conversationId })
}

// ─── Operation History ─────────────────────────────────

export interface OperationEntry {
  id: string
  session_id: string
  op_type: string
  tool_name: string
  input_preview: string
  success: boolean
  duration_ms: number
  timestamp: number
}

export interface OperationStats {
  total: number
  successful: number
  failed: number
  avg_duration_ms: number
  top_tools: Array<{ tool_name: string; count: number }>
}

export async function listOperations(sessionId: string): Promise<OperationEntry[]> {
  if (!isTauri()) return []
  return invoke<OperationEntry[]>("list_operations", { sessionId })
}

export async function getOperationStats(sessionId: string): Promise<OperationStats | null> {
  if (!isTauri()) return null
  return invoke<OperationStats>("get_operation_stats", { sessionId })
}
