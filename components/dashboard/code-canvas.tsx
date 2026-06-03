"use client"

import { useState, useEffect, useRef, useCallback } from "react"
import dynamic from "next/dynamic"
import {
  CheckCircle2, Play, Copy, X, ChevronDown, ChevronRight,
  FileCode, FileText, FolderOpen, Folder, Terminal as TerminalIcon,
  Search, GitBranch, Bug, Puzzle, MoreHorizontal, PanelBottomClose,
  PanelBottom, Split, Settings, Bell, Maximize2, Minimize2,
  Columns2, FilePlus, FolderPlus, RefreshCw, GripHorizontal
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { cn } from "@/lib/utils"
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend
} from "recharts"
import * as Y from "yjs"
import type { editor as monacoEditor } from "monaco-editor"
import { YjsWebsocketProvider } from "@/lib/yjs-provider"
import { getToken } from "@/lib/api"
import { useTauriAgent } from "@/hooks/use-tauri-agent"
import { isTauri } from "@/lib/tauri-api"
import { Terminal } from "@xterm/xterm"
import { FitAddon } from "@xterm/addon-fit"

// y-monaco accesses `window` at module level — must be lazy-loaded
let MonacoBindingModule: typeof import("y-monaco") | null = null
async function getMonacoBinding() {
  if (!MonacoBindingModule) {
    MonacoBindingModule = await import("y-monaco")
  }
  return MonacoBindingModule
}

const MonacoEditor = dynamic(
  () => import("@monaco-editor/react").then((mod) => mod.default),
  { ssr: false }
)

// File system structure
const fileSystem = {
  name: "sir-model",
  path: "",
  type: "folder" as const,
  children: [
    {
      name: "src",
      path: "src",
      type: "folder" as const,
      children: [
        { name: "model.py", path: "src/model.py", type: "file" as const, language: "python" },
        { name: "utils.py", path: "src/utils.py", type: "file" as const, language: "python" },
        { name: "config.json", path: "src/config.json", type: "file" as const, language: "json" },
      ]
    },
    {
      name: "tests",
      path: "tests",
      type: "folder" as const,
      children: [
        { name: "test_model.py", path: "tests/test_model.py", type: "file" as const, language: "python" },
      ]
    },
    { name: "requirements.txt", path: "requirements.txt", type: "file" as const, language: "plaintext" },
    { name: "README.md", path: "README.md", type: "file" as const, language: "markdown" },
  ]
}

// File contents
const fileContents: Record<string, string> = {
  "model.py": `import numpy as np
from scipy.integrate import odeint
import matplotlib.pyplot as plt

# SIR Model Parameters
beta = 0.3    # Transmission rate
gamma = 0.1   # Recovery rate
N = 1000      # Total population

# Initial conditions
S0, I0, R0 = 999, 1, 0

def sir_model(y, t, beta, gamma, N):
    S, I, R = y
    dSdt = -beta * S * I / N
    dIdt = beta * S * I / N - gamma * I
    dRdt = gamma * I
    return [dSdt, dIdt, dRdt]

# Time grid
t = np.linspace(0, 160, 160)

# Solve ODE
solution = odeint(sir_model, [S0, I0, R0], t, 
                  args=(beta, gamma, N))
S, I, R = solution.T

# Basic Reproduction Number
R0_value = beta / gamma
print(f"R₀ = {R0_value:.2f}")

# Plot results
plt.figure(figsize=(10, 6))
plt.plot(t, S, 'b-', label='Susceptible')
plt.plot(t, I, 'r-', label='Infected')
plt.plot(t, R, 'g-', label='Recovered')
plt.xlabel('Time (days)')
plt.ylabel('Population')
plt.title('SIR Model Simulation')
plt.legend()
plt.grid(alpha=0.3)
plt.show()`,
  "utils.py": `"""
Utility functions for SIR model analysis.
"""
import numpy as np

def calculate_r0(beta: float, gamma: float) -> float:
    """Calculate basic reproduction number."""
    return beta / gamma

def calculate_herd_immunity(r0: float) -> float:
    """Calculate herd immunity threshold."""
    return 1 - 1 / r0

def peak_infection_time(beta: float, gamma: float, S0: float, N: float) -> float:
    """Estimate time to peak infection."""
    r0 = calculate_r0(beta, gamma)
    return np.log(S0 / N * r0) / (beta - gamma)

def final_size(r0: float, N: float) -> float:
    """Calculate final epidemic size using transcendental equation."""
    from scipy.optimize import fsolve
    def equation(R_inf):
        return R_inf - N * (1 - np.exp(-r0 * R_inf / N))
    return fsolve(equation, N * 0.8)[0]`,
  "config.json": `{
  "model": {
    "type": "SIR",
    "parameters": {
      "beta": 0.3,
      "gamma": 0.1,
      "N": 1000
    },
    "initial_conditions": {
      "S0": 999,
      "I0": 1,
      "R0": 0
    }
  },
  "simulation": {
    "t_max": 160,
    "dt": 1,
    "solver": "odeint"
  },
  "output": {
    "format": "csv",
    "plot": true,
    "save_figures": true
  }
}`,
  "test_model.py": `import pytest
import numpy as np
from src.model import sir_model
from src.utils import calculate_r0, calculate_herd_immunity

class TestSIRModel:
    def test_conservation(self):
        """Test that S + I + R = N at all times."""
        beta, gamma, N = 0.3, 0.1, 1000
        y0 = [999, 1, 0]
        t = np.linspace(0, 100, 100)
        
        from scipy.integrate import odeint
        solution = odeint(sir_model, y0, t, args=(beta, gamma, N))
        
        totals = solution.sum(axis=1)
        np.testing.assert_array_almost_equal(totals, N)
    
    def test_r0_calculation(self):
        """Test R0 calculation."""
        assert calculate_r0(0.3, 0.1) == 3.0
        assert calculate_r0(0.5, 0.25) == 2.0
    
    def test_herd_immunity(self):
        """Test herd immunity threshold."""
        r0 = 3.0
        threshold = calculate_herd_immunity(r0)
        assert abs(threshold - 0.6667) < 0.001`,
  "requirements.txt": `numpy>=1.21.0
scipy>=1.7.0
matplotlib>=3.4.0
pytest>=6.2.0
pandas>=1.3.0`,
  "README.md": `# SIR Epidemic Model

A Python implementation of the classic SIR (Susceptible-Infected-Recovered) 
compartmental model for epidemic dynamics.

## Overview

The SIR model divides the population into three compartments:
- **S**usceptible: Individuals who can become infected
- **I**nfected: Individuals who are currently infected
- **R**ecovered: Individuals who have recovered and are immune

## Installation

\`\`\`bash
pip install -r requirements.txt
\`\`\`

## Usage

\`\`\`python
from src.model import sir_model
from scipy.integrate import odeint
import numpy as np

# Parameters
beta = 0.3  # Transmission rate
gamma = 0.1  # Recovery rate
N = 1000    # Population

# Solve
t = np.linspace(0, 160, 160)
solution = odeint(sir_model, [999, 1, 0], t, args=(beta, gamma, N))
\`\`\`

## License

MIT License`
}

// Generate SIR model data for the chart
const generateSIRData = () => {
  const beta = 0.3
  const gamma = 0.1
  const N = 1000
  let S = 999, I = 1, R = 0
  const data = []
  
  for (let t = 0; t <= 160; t += 2) {
    data.push({
      time: t,
      Susceptible: Math.round(S),
      Infected: Math.round(I),
      Recovered: Math.round(R),
    })
    
    const dS = -beta * S * I / N
    const dI = beta * S * I / N - gamma * I
    const dR = gamma * I
    
    S += dS
    I += dI
    R += dR
  }
  
  return data
}

const chartData = generateSIRData()
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

// Activity bar icons
const activityBarItems = [
  { icon: FileText, id: "explorer", label: "Explorer" },
  { icon: Search, id: "search", label: "Search" },
  { icon: GitBranch, id: "git", label: "Source Control" },
  { icon: Bug, id: "debug", label: "Run and Debug" },
  { icon: Puzzle, id: "extensions", label: "Extensions" },
]

type FileItem = {
  name: string
  path: string
  type: "file" | "folder"
  language?: string
  children?: FileItem[]
}

interface FileTreeItemProps {
  item: FileItem
  depth: number
  onFileSelect: (file: FileItem) => void
  selectedFile: string
}

function FileTreeItem({ item, depth, onFileSelect, selectedFile }: FileTreeItemProps) {
  const [isOpen, setIsOpen] = useState(depth < 2)
  const isFolder = item.type === "folder"
  const isSelected = !isFolder && item.path === selectedFile

  return (
    <div>
      <button
        onClick={() => isFolder ? setIsOpen(!isOpen) : onFileSelect(item)}
        className={cn(
          "w-full flex items-center gap-1 py-0.5 px-2 text-sm hover:bg-[#2a2d2e] transition-colors",
          isSelected && "bg-[#094771]"
        )}
        style={{ paddingLeft: `${depth * 12 + 8}px` }}
      >
        {isFolder ? (
          <>
            {isOpen ? <ChevronDown className="w-4 h-4 text-muted-foreground" /> : <ChevronRight className="w-4 h-4 text-muted-foreground" />}
            {isOpen ? <FolderOpen className="w-4 h-4 text-[#dcb67a]" /> : <Folder className="w-4 h-4 text-[#dcb67a]" />}
          </>
        ) : (
          <>
            <span className="w-4" />
            <FileCode className={cn(
              "w-4 h-4",
              item.name.endsWith('.py') && "text-[#4584b6]",
              item.name.endsWith('.json') && "text-[#cbcb41]",
              item.name.endsWith('.md') && "text-[#519aba]",
              item.name.endsWith('.txt') && "text-muted-foreground"
            )} />
          </>
        )}
        <span className={cn(
          "truncate text-[13px]",
          isSelected ? "text-white" : "text-[#cccccc]"
        )}>{item.name}</span>
      </button>
      {isFolder && isOpen && item.children?.map((child, i) => (
        <FileTreeItem 
          key={i} 
          item={child} 
          depth={depth + 1} 
          onFileSelect={onFileSelect}
          selectedFile={selectedFile}
        />
      ))}
    </div>
  )
}

interface TabItem {
  name: string
  path: string
  language: string
}

interface CodeCanvasProps {
  projectId: string
}

type AgentStatus = "connecting" | "connected" | "ready" | "disconnected"

type AgentMessage =
  | { type: "agent_status"; status: AgentStatus }
  | { type: "terminal_output"; data: string }
  | { type: "file_tree"; tree: FileItem }
  | { type: "file_content"; path: string; content: string }
  | { type: "file_change"; path: string; content: string }
  | { type: "work_dir"; path: string }
  | { type: "error"; message: string }

function FolderPathInput({
  onSubmit,
  onCancel,
  currentPath,
}: {
  onSubmit: (path: string) => void
  onCancel: () => void
  currentPath?: string
}) {
  const [value, setValue] = useState(currentPath || "")
  const inputRef = useRef<HTMLInputElement>(null)
  useEffect(() => { inputRef.current?.focus() }, [])

  const handleSubmit = () => {
    const trimmed = value.trim()
    if (trimmed) onSubmit(trimmed)
  }

  return (
    <div className="flex items-center gap-1 w-full px-2 py-1">
      <input
        ref={inputRef}
        type="text"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") handleSubmit()
          if (e.key === "Escape") onCancel()
        }}
        placeholder="D:/path/to/project"
        className="flex-1 bg-[#3c3c3c] text-[#cccccc] text-xs px-2 py-1 rounded border border-[#5a5a5a] outline-none focus:border-primary"
      />
      <button
        onClick={handleSubmit}
        className="text-xs text-primary hover:text-primary/80 px-1"
        title="Open"
      >
        OK
      </button>
      <button onClick={onCancel} className="text-xs text-muted-foreground hover:text-foreground px-1" title="Cancel">
        ✕
      </button>
    </div>
  )
}

export function CodeCanvas({ projectId }: CodeCanvasProps) {
  // Yjs CRDT state
  const yDocsRef = useRef<Map<string, { doc: Y.Doc; provider: YjsWebsocketProvider }>>(new Map())
  const bindingsRef = useRef<Map<string, { destroy: () => void }>>(new Map())
  const editorRef = useRef<monacoEditor.IStandaloneCodeEditor | null>(null)
  const agentWsRef = useRef<WebSocket | null>(null)
  const xtermRef = useRef<Terminal | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const pendingTerminalWritesRef = useRef<string[]>([])
  const isTauriMode = useRef(isTauri())
  const [editorReady, setEditorReady] = useState(false)
  const [agentStatus, setAgentStatus] = useState<AgentStatus>("connecting")
  const [agentFileTree, setAgentFileTree] = useState<FileItem | null>(null)
  const [agentFileContents, setAgentFileContents] = useState<Record<string, string>>({})

  // Tauri Agent (replaces WebSocket agent when running in Tauri)
  const tauriAgent = useTauriAgent()

  const [activeActivity, setActiveActivity] = useState("explorer")
  const [openTabs, setOpenTabs] = useState<TabItem[]>([
    { name: "model.py", path: "src/model.py", language: "python" }
  ])
  const [activeTab, setActiveTab] = useState("src/model.py")
  const activeTabRef = useRef(activeTab)
  const [showTerminal, setShowTerminal] = useState(true)
  const [isRunning, setIsRunning] = useState(false)
  const [copied, setCopied] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [isMaximized, setIsMaximized] = useState(false)
  const [activePanel, setActivePanel] = useState<"terminal" | "output" | "problems">("terminal")
  const [terminalHeight, setTerminalHeight] = useState(40) // percentage
  const [isDragging, setIsDragging] = useState(false)
  const [splitView, setSplitView] = useState(false)
  const terminalContainerRef = useRef<HTMLDivElement>(null)
  const editorContainerRef = useRef<HTMLDivElement>(null)

  const sendAgentMessage = (message: unknown) => {
    if (isTauriMode.current) {
      const msg = message as {
        type: string
        data?: string
        path?: string
        content?: string
        cols?: number
        rows?: number
      }
      switch (msg.type) {
        case "terminal_input":
          if (msg.data) tauriAgent.writeToTerminal(msg.data)
          break
        case "terminal_resize":
          if (msg.cols && msg.rows) tauriAgent.resizeTerminal(msg.cols, msg.rows)
          break
        case "open_file":
          if (msg.path) tauriAgent.openFile(msg.path)
          break
        case "list_files":
          tauriAgent.refreshFileTree()
          break
        case "change_work_dir":
          if (msg.path) tauriAgent.changeDir(msg.path)
          break
        case "create_file":
          if (msg.path && msg.content) tauriAgent.createFile(msg.path, msg.content)
          break
        case "new_file":
        case "new_folder":
          writeTerminal("\r\n[agent] New File/Folder: use the terminal\r\n")
          break
      }
      return true
    }
    if (agentWsRef.current?.readyState === WebSocket.OPEN) {
      agentWsRef.current.send(JSON.stringify(message))
      return true
    }
    writeTerminal("\r\n[agent] local agent websocket is not connected\r\n")
    return false
  }

  const writeTerminal = (data: string) => {
    const terminal = xtermRef.current
    if (terminal) {
      terminal.write(data)
    } else {
      pendingTerminalWritesRef.current.push(data)
    }
  }

  const fitTerminal = () => {
    const terminal = xtermRef.current
    const fitAddon = fitAddonRef.current
    if (!terminal || !fitAddon) return
    fitAddon.fit()
    if (isTauriMode.current) {
      tauriAgent.resizeTerminal(terminal.cols, terminal.rows)
    } else if (agentWsRef.current?.readyState === WebSocket.OPEN) {
      agentWsRef.current.send(JSON.stringify({
        type: "terminal_resize",
        cols: terminal.cols,
        rows: terminal.rows,
      }))
    }
  }

  const handleFileSelect = (file: FileItem) => {
    if (!openTabs.find(t => t.path === file.path)) {
      const extension = file.name.split('.').pop()
      const language = file.language || (extension === 'py' ? 'python' :
                      extension === 'json' ? 'json' :
                      extension === 'md' ? 'markdown' : 'plaintext')
      setOpenTabs([...openTabs, { name: file.name, path: file.path, language }])
    }
    setActiveTab(file.path)
    sendAgentMessage({ type: "open_file", path: file.path })
  }

  useEffect(() => {
    activeTabRef.current = activeTab
  }, [activeTab])

  useEffect(() => {
    const container = terminalContainerRef.current
    if (!container || xtermRef.current) return

    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: true,
      fontFamily: "var(--font-mono), Consolas, 'Courier New', monospace",
      fontSize: 13,
      scrollback: 5000,
      theme: {
        background: "#1e1e1e",
        foreground: "#cccccc",
        cursor: "#ffffff",
        selectionBackground: "#264f78",
      },
    })
    const fitAddon = new FitAddon()
    terminal.loadAddon(fitAddon)
    terminal.open(container)
    xtermRef.current = terminal
    fitAddonRef.current = fitAddon
    const resizeObserver = new ResizeObserver(() => {
      requestAnimationFrame(fitTerminal)
    })
    resizeObserver.observe(container)

    terminal.onData((data) => {
      sendAgentMessage({ type: "terminal_input", data })
    })

    for (const data of pendingTerminalWritesRef.current) {
      terminal.write(data)
    }
    pendingTerminalWritesRef.current = []
    terminal.writeln("Connecting to local agent...")
    requestAnimationFrame(fitTerminal)

    return () => {
      resizeObserver.disconnect()
      terminal.dispose()
      xtermRef.current = null
      fitAddonRef.current = null
    }
  }, [])

  useEffect(() => {
    requestAnimationFrame(fitTerminal)
  }, [isMaximized, showTerminal, activePanel, sidebarCollapsed])

  useEffect(() => {
    const token = getToken()
    if (!token || !projectId) {
      setAgentStatus("disconnected")
      writeTerminal("\r\n[agent] sign in before connecting the local agent\r\n")
      return
    }

    const base = process.env.NEXT_PUBLIC_WS_URL || "ws://localhost:3001"
    const url = `${base}/agent?role=frontend&project_id=${encodeURIComponent(projectId)}&token=${encodeURIComponent(token)}`
    const ws = new WebSocket(url)
    agentWsRef.current = ws
    setAgentStatus("connecting")

    ws.onopen = () => {
      writeTerminal("\r\n[agent] frontend websocket connected\r\n")
      ws.send(JSON.stringify({ type: "list_files" }))
      ws.send(JSON.stringify({ type: "open_file", path: activeTabRef.current }))
      fitTerminal()
    }
    ws.onmessage = (event) => {
      const msg = JSON.parse(event.data) as AgentMessage
      if (msg.type === "agent_status") {
        setAgentStatus(msg.status)
        writeTerminal(`\r\n[agent] ${msg.status}\r\n`)
        if (msg.status === "connected" || msg.status === "ready") {
          ws.send(JSON.stringify({ type: "list_files" }))
          ws.send(JSON.stringify({ type: "open_file", path: activeTabRef.current }))
          fitTerminal()
        }
      } else if (msg.type === "terminal_output") {
        writeTerminal(msg.data)
      } else if (msg.type === "file_tree") {
        setAgentFileTree(msg.tree)
      } else if (msg.type === "file_content") {
        setAgentFileContents((prev) => ({ ...prev, [msg.path]: msg.content }))
      } else if (msg.type === "file_change") {
        setAgentFileContents((prev) => ({ ...prev, [msg.path]: msg.content }))
      } else if (msg.type === "work_dir") {
        setWorkDir(msg.path)
        writeTerminal(`\r\n[agent] Working directory: ${msg.path}\r\n`)
      } else if (msg.type === "error") {
        writeTerminal(`\r\n[agent] ${msg.message}\r\n`)
      }
    }
    ws.onclose = () => {
      if (agentWsRef.current === ws) {
        setAgentStatus("disconnected")
        writeTerminal("\r\n[agent] frontend websocket disconnected\r\n")
      }
    }
    ws.onerror = () => {
      setAgentStatus("disconnected")
      writeTerminal("\r\n[agent] websocket error\r\n")
    }

    return () => {
      if (agentWsRef.current === ws) {
        agentWsRef.current = null
      }
      ws.close()
    }
  }, [projectId])

  // Tauri Agent: connect on mount, sync state to existing UI
  useEffect(() => {
    if (!isTauriMode.current) return
    tauriAgent.connect()
    return () => {
      tauriAgent.disconnect()
    }
  }, [])

  useEffect(() => {
    if (!isTauriMode.current) return
    return tauriAgent.onTerminalData((data) => {
      writeTerminal(data)
    })
  }, [tauriAgent])

  useEffect(() => {
    if (!isTauriMode.current || !tauriAgent.fileTree) return
    setAgentFileTree(tauriAgent.fileTree)
  }, [tauriAgent.fileTree])

  useEffect(() => {
    if (!isTauriMode.current) return
    setAgentFileContents(tauriAgent.fileContents)
  }, [tauriAgent.fileContents])

  useEffect(() => {
    if (!isTauriMode.current) return
    setAgentStatus(tauriAgent.status)
  }, [tauriAgent.status])

  useEffect(() => {
    if (!isTauriMode.current || !tauriAgent.workDir) return
    setWorkDir(tauriAgent.workDir)
  }, [tauriAgent.workDir])

  const handleCloseTab = (fileName: string, e: React.MouseEvent) => {
    e.stopPropagation()
    const newTabs = openTabs.filter(t => t.path !== fileName)
    setOpenTabs(newTabs)
    if (activeTab === fileName && newTabs.length > 0) {
      setActiveTab(newTabs[newTabs.length - 1].path)
    }
  }

  const handleCopy = () => {
    const activeName = openTabs.find(t => t.path === activeTab)?.name || activeTab
    const content = agentFileContents[activeTab] || fileContents[activeTab] || fileContents[activeName] || ""
    navigator.clipboard.writeText(content)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  const handleRun = () => {
    setIsRunning(true)
    setActivePanel("terminal")
    setShowTerminal(true)
    sendAgentMessage({ type: "terminal_input", data: `python ${activeTab}\r` })
    setTimeout(() => setIsRunning(false), 500)
  }

  // Set up Monaco/Yjs binding when the editor or active file changes.
  useEffect(() => {
    const editor = editorRef.current
    if (!editorReady || !editor || !activeTab) return

    bindingsRef.current.forEach((binding) => binding.destroy())
    bindingsRef.current.clear()

    const syncFileId = UUID_RE.test(activeTab) ? activeTab : null
    if (!syncFileId) {
      const activeName = openTabs.find(t => t.path === activeTab)?.name || activeTab
      editor.setValue(agentFileContents[activeTab] || fileContents[activeTab] || fileContents[activeName] || "// File not found")
      return
    }

    let entry = yDocsRef.current.get(syncFileId)
    if (!entry) {
      const doc = new Y.Doc()
      const provider = new YjsWebsocketProvider(doc, syncFileId)
      entry = { doc, provider }
      yDocsRef.current.set(syncFileId, entry)
    }

    const yText = entry.doc.getText("content")
    if (yText.length === 0) {
      yText.insert(0, agentFileContents[activeTab] || fileContents[activeTab] || "")
    } else {
      editor.setValue(yText.toString())
    }

    getMonacoBinding().then(({ MonacoBinding }) => {
      if (!editorRef.current || activeTabRef.current !== activeTab) return
      const binding = new MonacoBinding(
        yText,
        editor.getModel()!,
        new Set([editor]),
        undefined
      )
      bindingsRef.current.set(syncFileId, binding)
    })

    return () => {
      const existing = bindingsRef.current.get(syncFileId)
      if (existing) {
        existing.destroy()
        bindingsRef.current.delete(syncFileId)
      }
    }
  }, [activeTab, editorReady, agentFileContents, openTabs])

  // Cleanup Yjs on unmount
  useEffect(() => {
    return () => {
      bindingsRef.current.forEach(b => b.destroy())
      yDocsRef.current.forEach(({ provider }) => provider.destroy())
    }
  }, [])

  // Terminal drag resize handler
  const handleDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setIsDragging(true)
    const startY = e.clientY
    const startHeight = terminalHeight
    const containerEl = editorContainerRef.current
    const containerHeight = containerEl?.getBoundingClientRect().height || 600

    const onMouseMove = (ev: MouseEvent) => {
      const deltaY = startY - ev.clientY
      const newPct = Math.min(70, Math.max(15, startHeight + (deltaY / containerHeight) * 100))
      setTerminalHeight(Math.round(newPct))
    }
    const onMouseUp = () => {
      setIsDragging(false)
      document.removeEventListener("mousemove", onMouseMove)
      document.removeEventListener("mouseup", onMouseUp)
    }
    document.addEventListener("mousemove", onMouseMove)
    document.addEventListener("mouseup", onMouseUp)
  }, [terminalHeight])

  // Explorer context menu click-outside
  const [explorerMenuOpen, setExplorerMenuOpen] = useState(false)
  const [showOpenFolder, setShowOpenFolder] = useState(false)
  const [workDir, setWorkDir] = useState<string | null>(null)
  const explorerMenuRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (explorerMenuRef.current && !explorerMenuRef.current.contains(e.target as Node)) {
        setExplorerMenuOpen(false)
      }
    }
    if (explorerMenuOpen) document.addEventListener("mousedown", onClick)
    return () => document.removeEventListener("mousedown", onClick)
  }, [explorerMenuOpen])

  const currentLanguage = openTabs.find(t => t.path === activeTab)?.language || "plaintext"
  const currentFileTree = agentFileTree || fileSystem

  const handleEditorMount = (editor: monacoEditor.IStandaloneCodeEditor, _monaco: typeof import("monaco-editor")) => {
    editorRef.current = editor
    setEditorReady(true)
  }

  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      editorRef.current?.layout()
    })
    return () => cancelAnimationFrame(frame)
  }, [isMaximized, showTerminal, sidebarCollapsed])

  return (
    <div
      className={cn(
        "h-full flex flex-col bg-[#1e1e1e] border-l border-[#3c3c3c]",
        isMaximized && "fixed inset-0 z-50 border-l-0"
      )}
    >
      {/* Title Bar */}
      <div className="h-8 bg-[#323233] flex items-center justify-between px-2 border-b border-[#3c3c3c]">
        <div className="flex items-center gap-2">
          <div className={cn(
            "flex items-center gap-1.5 px-2 py-0.5 rounded text-xs",
            agentStatus === "ready" ? "bg-green-500/20 text-green-400" :
            agentStatus === "connected" ? "bg-primary/10 text-primary" :
            agentStatus === "connecting" ? "bg-yellow-500/20 text-yellow-400" :
            "bg-red-500/20 text-red-400"
          )}>
            <div className={cn(
              "w-2 h-2 rounded-full",
              agentStatus === "ready" ? "bg-green-400" :
              agentStatus === "connected" ? "bg-primary" :
              agentStatus === "connecting" ? "bg-yellow-400 animate-pulse" :
              "bg-red-400"
            )} />
            <span className="capitalize">{agentStatus}</span>
          </div>
          <span className="text-xs text-muted-foreground">Modeler AI</span>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6 text-muted-foreground hover:text-foreground"
            onClick={() => setIsMaximized(false)}
            disabled={!isMaximized}
            title="Restore editor"
          >
            <Minimize2 className="w-3.5 h-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6 text-muted-foreground hover:text-foreground"
            onClick={() => setIsMaximized((prev) => !prev)}
            title={isMaximized ? "Restore editor" : "Maximize editor"}
          >
            {isMaximized ? <Minimize2 className="w-3.5 h-3.5" /> : <Maximize2 className="w-3.5 h-3.5" />}
          </Button>
        </div>
      </div>

      <div className="flex-1 flex min-h-0">
        {/* Activity Bar */}
        <div className="w-12 bg-[#333333] flex flex-col items-center py-2 border-r border-[#3c3c3c]">
          {activityBarItems.map((item) => (
            <button
              key={item.id}
              onClick={() => {
                if (activeActivity === item.id) {
                  setSidebarCollapsed(!sidebarCollapsed)
                } else {
                  setActiveActivity(item.id)
                  setSidebarCollapsed(false)
                }
              }}
              className={cn(
                "w-12 h-12 flex items-center justify-center transition-colors relative",
                activeActivity === item.id && !sidebarCollapsed
                  ? "text-white"
                  : "text-[#858585] hover:text-white"
              )}
              title={item.label}
            >
              <item.icon className="w-6 h-6" />
              {activeActivity === item.id && !sidebarCollapsed && (
                <div className="absolute left-0 top-0 bottom-0 w-0.5 bg-white" />
              )}
            </button>
          ))}
          <div className="flex-1" />
          <button className="w-12 h-12 flex items-center justify-center text-[#858585] hover:text-white transition-colors">
            <Settings className="w-5 h-5" />
          </button>
        </div>

        {/* Side Bar (File Explorer) */}
        {!sidebarCollapsed && (
          <div className="w-56 bg-[#252526] border-r border-[#3c3c3c] flex flex-col min-h-0">
            <div className="h-9 flex-shrink-0 px-4 flex items-center justify-between border-b border-[#3c3c3c] gap-1">
              <span className="text-[11px] font-medium text-[#bbbbbb] uppercase tracking-wider truncate" title={workDir || undefined}>
                {workDir ? workDir.split(/[/\\]/).pop() || "Explorer" : "Explorer"}
              </span>
              <div className="relative flex items-center gap-0.5" ref={explorerMenuRef}>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-5 w-5 text-muted-foreground hover:text-foreground"
                  onClick={() => setShowOpenFolder(!showOpenFolder)}
                  title="Open Folder"
                >
                  <FolderOpen className="w-3.5 h-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-5 w-5 text-muted-foreground hover:text-foreground"
                  onClick={() => setExplorerMenuOpen(!explorerMenuOpen)}
                  title="More"
                >
                  <MoreHorizontal className="w-4 h-4" />
                </Button>
                {showOpenFolder && (
                  <div className="absolute right-0 top-6 w-64 bg-[#252526] border border-[#3c3c3c] rounded-md shadow-lg z-50">
                    <FolderPathInput
                      currentPath={workDir || undefined}
                      onSubmit={(path) => {
                        setShowOpenFolder(false)
                        sendAgentMessage({ type: "change_work_dir", path })
                        setWorkDir(path)
                      }}
                      onCancel={() => setShowOpenFolder(false)}
                    />
                  </div>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-5 w-5 text-muted-foreground hover:text-foreground"
                  onClick={() => setExplorerMenuOpen(!explorerMenuOpen)}
                >
                  <MoreHorizontal className="w-4 h-4" />
                </Button>
                {explorerMenuOpen && (
                  <div className="absolute right-0 top-6 w-40 bg-[#252526] border border-[#3c3c3c] rounded-md shadow-lg z-50 py-1 text-xs">
                    <button
                      className="w-full text-left px-3 py-1.5 text-[#cccccc] hover:bg-[#2a2d2e] flex items-center gap-2"
                      onClick={() => { setExplorerMenuOpen(false); sendAgentMessage({ type: "new_file" }) }}
                    >
                      <FilePlus className="w-3.5 h-3.5" /> New File
                    </button>
                    <button
                      className="w-full text-left px-3 py-1.5 text-[#cccccc] hover:bg-[#2a2d2e] flex items-center gap-2"
                      onClick={() => { setExplorerMenuOpen(false); sendAgentMessage({ type: "new_folder" }) }}
                    >
                      <FolderPlus className="w-3.5 h-3.5" /> New Folder
                    </button>
                    <div className="border-t border-[#3c3c3c] my-0.5" />
                    <button
                      className="w-full text-left px-3 py-1.5 text-[#cccccc] hover:bg-[#2a2d2e] flex items-center gap-2"
                      onClick={() => { setExplorerMenuOpen(false); sendAgentMessage({ type: "list_files" }) }}
                    >
                      <RefreshCw className="w-3.5 h-3.5" /> Refresh
                    </button>
                  </div>
                )}
              </div>
            </div>
            <ScrollArea className="flex-1 min-h-0">
              <div className="py-1">
                <FileTreeItem
                  item={currentFileTree}
                  depth={0}
                  onFileSelect={handleFileSelect}
                  selectedFile={activeTab}
                />
              </div>
            </ScrollArea>
          </div>
        )}

        {/* Editor Area */}
        <div className="flex-1 flex flex-col min-w-0">
          {/* Tabs */}
          <div className="h-9 bg-[#252526] flex items-center border-b border-[#3c3c3c] overflow-x-auto">
            {openTabs.map((tab) => (
              <div
                key={tab.path}
                onClick={() => setActiveTab(tab.path)}
                className={cn(
                  "h-full flex items-center gap-2 px-3 cursor-pointer border-r border-[#3c3c3c] group min-w-0",
                  activeTab === tab.path 
                    ? "bg-[#1e1e1e] text-white" 
                    : "bg-[#2d2d2d] text-[#969696] hover:bg-[#2d2d2d]"
                )}
              >
                <FileCode className={cn(
                  "w-4 h-4 shrink-0",
                  tab.name.endsWith('.py') && "text-[#4584b6]",
                  tab.name.endsWith('.json') && "text-[#cbcb41]",
                  tab.name.endsWith('.md') && "text-[#519aba]"
                )} />
                <span className="text-[13px] truncate">{tab.name}</span>
                <button
                  onClick={(e) => handleCloseTab(tab.path, e)}
                  className="opacity-0 group-hover:opacity-100 hover:bg-[#3c3c3c] rounded p-0.5 transition-opacity shrink-0"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            ))}
            <div className="flex-1" />
            <div className="flex items-center gap-1 px-2">
              <Button 
                variant="ghost" 
                size="icon" 
                className="h-6 w-6 text-muted-foreground hover:text-foreground"
                onClick={handleCopy}
              >
                <Copy className="w-3.5 h-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className={cn(
                  "h-6 w-6 hover:text-foreground",
                  splitView ? "text-primary" : "text-muted-foreground"
                )}
                onClick={() => setSplitView(!splitView)}
                title="Toggle split editor"
              >
                <Columns2 className="w-3.5 h-3.5" />
              </Button>
              <Button 
                size="sm" 
                className="h-6 text-xs bg-[#0e639c] hover:bg-[#1177bb] text-white"
                onClick={handleRun}
                disabled={isRunning}
              >
                <Play className="w-3 h-3 mr-1" />
                {isRunning ? "Running..." : "Run"}
              </Button>
            </div>
          </div>

          {/* Breadcrumb */}
          <div className="h-6 bg-[#1e1e1e] flex items-center px-3 text-xs text-muted-foreground border-b border-[#3c3c3c] min-w-0">
            {(() => {
              const parts = activeTab.split("/")
              const filename = parts.pop() || activeTab
              return (
                <span className="truncate">
                  {parts.length > 0 && parts.map((segment, i) => (
                    <span key={i}>
                      <span className="text-[#858585]">{segment}</span>
                      <ChevronRight className="w-3 h-3 mx-0.5 inline text-[#5a5a5a]" />
                    </span>
                  ))}
                  <span className="text-foreground">{filename}</span>
                </span>
              )
            })()}
          </div>

          {/* Editor + Terminal */}
          <div ref={editorContainerRef} className="flex-1 flex flex-col min-h-0">
            {/* Monaco Editor */}
            <div
              className={cn("min-h-0", showTerminal ? "" : "flex-1")}
              style={showTerminal ? { height: `${100 - terminalHeight}%` } : undefined}
            >
              <MonacoEditor
                height="100%"
                language={currentLanguage}
                theme="vs-dark"
                onMount={handleEditorMount}
                options={{
                  minimap: { enabled: true, scale: 0.8 },
                  fontSize: 13,
                  lineNumbers: "on",
                  scrollBeyondLastLine: false,
                  padding: { top: 8, bottom: 8 },
                  fontFamily: "var(--font-mono), 'Fira Code', monospace",
                  renderLineHighlight: "all",
                  cursorBlinking: "smooth",
                  smoothScrolling: true,
                  bracketPairColorization: { enabled: true },
                  guides: {
                    bracketPairs: true,
                    indentation: true,
                  },
                  renderWhitespace: "selection",
                  wordWrap: "on",
                }}
              />
            </div>

            {/* Drag handle */}
            {showTerminal && (
              <div
                className={cn(
                  "h-1 bg-[#3c3c3c] hover:bg-primary/60 cursor-ns-resize transition-colors flex-shrink-0 z-10",
                  isDragging && "bg-primary"
                )}
                onMouseDown={handleDragStart}
              >
                <div className="w-8 h-1 mx-auto mt-px rounded bg-muted-foreground/30" />
              </div>
            )}

            {/* Terminal Panel */}
            {showTerminal && (
              <div
                className="min-h-[100px] border-t-0 flex flex-col bg-[#1e1e1e]"
                style={{ height: `${terminalHeight}%` }}
              >
                {/* Panel Header */}
                <div className="h-9 flex items-center justify-between px-2 bg-[#252526] border-b border-[#3c3c3c]">
                  <div className="flex items-center gap-1">
                    {["problems", "output", "terminal"].map((panel) => (
                      <button
                        key={panel}
                        onClick={() => setActivePanel(panel as typeof activePanel)}
                        className={cn(
                          "px-3 py-1 text-xs uppercase tracking-wider transition-colors",
                          activePanel === panel 
                            ? "text-white border-b-2 border-primary" 
                            : "text-[#858585] hover:text-white"
                        )}
                      >
                        {panel}
                        {panel === "problems" && (
                          <span className="ml-1.5 px-1.5 py-0.5 rounded-full bg-[#3c3c3c] text-[10px]">0</span>
                        )}
                      </button>
                    ))}
                  </div>
                  <div className="flex items-center gap-1">
                    <Button 
                      variant="ghost" 
                      size="icon" 
                      className="h-6 w-6 text-muted-foreground hover:text-foreground"
                      onClick={() => setShowTerminal(false)}
                    >
                      <PanelBottomClose className="w-4 h-4" />
                    </Button>
                  </div>
                </div>

                {/* Terminal Content */}
                <div className={cn("flex-1 flex flex-col min-h-0", activePanel !== "terminal" && "hidden")}>
                    <div className="h-7 flex items-center gap-2 px-3 bg-[#1e1e1e] border-b border-[#3c3c3c]">
                      <TerminalIcon className="w-3.5 h-3.5 text-muted-foreground" />
                      <span className="text-xs text-muted-foreground">local shell</span>
                      <span className="text-xs text-muted-foreground">- {agentStatus}</span>
                    </div>
                    <div ref={terminalContainerRef} className="flex-1 min-h-0 p-2 [&_.xterm]:h-full" />
                  </div>

                {activePanel === "output" && (
                  <div className="flex-1 p-4 overflow-auto">
                    <div className="text-xs font-medium text-muted-foreground mb-3">SIR Model Output</div>
                    <div className="h-48 bg-[#252526] rounded-lg p-2">
                      <ResponsiveContainer width="100%" height="100%">
                        <LineChart data={chartData}>
                          <CartesianGrid strokeDasharray="3 3" stroke="#3c3c3c" />
                          <XAxis 
                            dataKey="time" 
                            stroke="#858585"
                            tick={{ fontSize: 10 }}
                          />
                          <YAxis 
                            stroke="#858585"
                            tick={{ fontSize: 10 }}
                          />
                          <Tooltip 
                            contentStyle={{ 
                              backgroundColor: '#252526', 
                              border: '1px solid #3c3c3c',
                              borderRadius: '4px',
                              fontSize: 11
                            }}
                          />
                          <Legend wrapperStyle={{ fontSize: 10 }} />
                          <Line type="monotone" dataKey="Susceptible" stroke="#569cd6" strokeWidth={2} dot={false} />
                          <Line type="monotone" dataKey="Infected" stroke="#f14c4c" strokeWidth={2} dot={false} />
                          <Line type="monotone" dataKey="Recovered" stroke="#4ec9b0" strokeWidth={2} dot={false} />
                        </LineChart>
                      </ResponsiveContainer>
                    </div>
                  </div>
                )}

                {activePanel === "problems" && (
                  <div className="flex-1 flex items-center justify-center text-sm text-muted-foreground">
                    No problems detected in workspace
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Status Bar */}
      <div className="h-6 bg-[#007acc] flex items-center justify-between px-2 text-xs text-white">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-1">
            <GitBranch className="w-3.5 h-3.5" />
            <span>main</span>
          </div>
          <div className="flex items-center gap-1">
            <span className="w-2 h-2 rounded-full bg-green-400" />
            <span>0 Errors</span>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <span>Python 3.11.4</span>
          <span>UTF-8</span>
          <span>LF</span>
          {!showTerminal && (
            <button 
              onClick={() => setShowTerminal(true)}
              className="flex items-center gap-1 hover:bg-white/10 px-1 rounded"
            >
              <PanelBottom className="w-3.5 h-3.5" />
              <span>Terminal</span>
            </button>
          )}
          <Bell className="w-3.5 h-3.5" />
        </div>
      </div>
    </div>
  )
}
