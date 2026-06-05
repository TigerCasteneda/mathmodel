"use client"

import { useState, useEffect, useRef, useCallback } from "react"
import type { FileTreeItem } from "@/lib/tauri-api"
import * as tauriApi from "@/lib/tauri-api"

export type AgentStatus = "connecting" | "connected" | "ready" | "disconnected"

export function useTauriAgent() {
  const [status, setStatus] = useState<AgentStatus>("disconnected")
  const [fileTree, setFileTree] = useState<FileTreeItem | null>(null)
  const [fileContents, setFileContents] = useState<Record<string, string>>({})
  const [workDir, setWorkDir] = useState<string | null>(null)
  const terminalCallbackRef = useRef<((data: string) => void) | null>(null)
  const cleanupRef = useRef<(() => void) | null>(null)

  const connect = useCallback(async () => {
    console.log("[agent] connect() called, isTauri:", tauriApi.isTauri())
    if (!tauriApi.isTauri()) {
      console.log("[agent] not in Tauri, skipping connect")
      return
    }
    setStatus("connecting")

    try {
      setStatus("ready")

      const unlisteners: (() => void)[] = []

      unlisteners.push(
        tauriApi.onAgentError((message) => {
          terminalCallbackRef.current?.(`\r\n[agent] ${message}\r\n`)
        })
      )

      unlisteners.push(
        tauriApi.onFileChange((path, content) => {
          setFileContents((prev) => ({ ...prev, [path]: content }))
        })
      )

      unlisteners.push(
        tauriApi.onFileTree((tree) => {
          setFileTree(tree)
        })
      )

      unlisteners.push(
        tauriApi.onFileContent((path, content) => {
          setFileContents((prev) => ({ ...prev, [path]: content }))
        })
      )

      unlisteners.push(
        tauriApi.onWorkDirChanged((path) => {
          setWorkDir(path)
        })
      )

      cleanupRef.current = () => {
        unlisteners.forEach((fn) => fn())
      }

      const tree = await tauriApi.listFiles()
      setFileTree(tree)
    } catch (err) {
      setStatus("disconnected")
      console.error("[agent] Failed to connect agent:", err)
    }
  }, [])

  const disconnect = useCallback(() => {
    cleanupRef.current?.()
    setStatus("disconnected")
  }, [])

  useEffect(() => {
    return () => {
      cleanupRef.current?.()
    }
  }, [])

  const writeToTerminal = useCallback((data: string) => {
    terminalCallbackRef.current?.(`\r\n[agent] PTY is retired in Phase 9. Ignored input: ${data}\r\n`)
  }, [])

  const resizeTerminal = useCallback((cols: number, rows: number) => {
    void cols
    void rows
  }, [])

  const openFile = useCallback(async (path: string) => {
    try {
      const content = await tauriApi.readFile(path)
      setFileContents((prev) => ({ ...prev, [path]: content }))
      return content
    } catch {
      return null
    }
  }, [])

  const createFile_ = useCallback(async (path: string, content: string) => {
    await tauriApi.createFile(path, content)
  }, [])

  const openFolder = useCallback(async () => {
    const path = await tauriApi.openFolder()
    if (!path) return null
    setWorkDir(path)
    const tree = await tauriApi.listFiles()
    setFileTree(tree)
    return path
  }, [])

  const changeDir = useCallback(async (path: string) => {
    const tree = await tauriApi.changeWorkDir(path)
    setFileTree(tree)
    setWorkDir(path)
  }, [])

  const refreshFileTree = useCallback(async () => {
    try {
      const tree = await tauriApi.listFiles()
      setFileTree(tree)
    } catch {
      /* ignore */
    }
  }, [])

  const writeFile = useCallback(async (path: string, content: string) => {
    await tauriApi.writeFile(path, content)
    setFileContents((prev) => ({ ...prev, [path]: content }))
    await refreshFileTree()
  }, [refreshFileTree])

  const onTerminalData = useCallback((callback: (data: string) => void) => {
    terminalCallbackRef.current = callback
    return () => {
      terminalCallbackRef.current = null
    }
  }, [])

  return {
    status,
    fileTree,
    fileContents,
    workDir,
    connect,
    disconnect,
    writeToTerminal,
    resizeTerminal,
    openFile,
    createFile: createFile_,
    writeFile,
    openFolder,
    changeDir,
    refreshFileTree,
    onTerminalData,
  }
}
