"use client"

import { useState, useEffect, useRef } from "react"
import dynamic from "next/dynamic"
import { 
  CheckCircle2, Play, Copy, X, ChevronDown, ChevronRight,
  FileCode, FileText, FolderOpen, Folder, Terminal as TerminalIcon,
  Search, GitBranch, Bug, Puzzle, MoreHorizontal, PanelBottomClose,
  PanelBottom, Split, Settings, Bell, Maximize2, Minimize2
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
import { MonacoBinding } from "y-monaco"
import type { editor as monacoEditor } from "monaco-editor"
import { YjsWebsocketProvider } from "@/lib/yjs-provider"

const MonacoEditor = dynamic(
  () => import("@monaco-editor/react").then((mod) => mod.default),
  { ssr: false }
)

// File system structure
const fileSystem = {
  name: "sir-model",
  type: "folder" as const,
  children: [
    {
      name: "src",
      type: "folder" as const,
      children: [
        { name: "model.py", type: "file" as const, language: "python" },
        { name: "utils.py", type: "file" as const, language: "python" },
        { name: "config.json", type: "file" as const, language: "json" },
      ]
    },
    {
      name: "tests",
      type: "folder" as const,
      children: [
        { name: "test_model.py", type: "file" as const, language: "python" },
      ]
    },
    { name: "requirements.txt", type: "file" as const, language: "plaintext" },
    { name: "README.md", type: "file" as const, language: "markdown" },
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
  type: "file" | "folder"
  language?: string
  children?: FileItem[]
}

interface FileTreeItemProps {
  item: FileItem
  depth: number
  onFileSelect: (fileName: string) => void
  selectedFile: string
}

function FileTreeItem({ item, depth, onFileSelect, selectedFile }: FileTreeItemProps) {
  const [isOpen, setIsOpen] = useState(depth < 2)
  const isFolder = item.type === "folder"
  const isSelected = !isFolder && item.name === selectedFile

  return (
    <div>
      <button
        onClick={() => isFolder ? setIsOpen(!isOpen) : onFileSelect(item.name)}
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
  language: string
}

export function CodeCanvas() {
  // Yjs CRDT state
  const yDocsRef = useRef<Map<string, { doc: Y.Doc; provider: YjsWebsocketProvider }>>(new Map())
  const bindingsRef = useRef<Map<string, MonacoBinding>>(new Map())
  const editorRef = useRef<monacoEditor.IStandaloneCodeEditor | null>(null)
  const [editorReady, setEditorReady] = useState(false)

  const [activeActivity, setActiveActivity] = useState("explorer")
  const [openTabs, setOpenTabs] = useState<TabItem[]>([
    { name: "model.py", language: "python" }
  ])
  const [activeTab, setActiveTab] = useState("model.py")
  const [showTerminal, setShowTerminal] = useState(true)
  const [terminalOutput, setTerminalOutput] = useState<string[]>([
    "Python 3.11.4 (main, Jun  9 2024, 07:31:04)",
    '>>> exec(open("src/model.py").read())',
    "R₀ = 3.00",
    ">>> "
  ])
  const [terminalInput, setTerminalInput] = useState("")
  const [isRunning, setIsRunning] = useState(false)
  const [copied, setCopied] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const [activePanel, setActivePanel] = useState<"terminal" | "output" | "problems">("terminal")
  const terminalRef = useRef<HTMLDivElement>(null)

  const handleFileSelect = (fileName: string) => {
    if (!openTabs.find(t => t.name === fileName)) {
      const extension = fileName.split('.').pop()
      const language = extension === 'py' ? 'python' : 
                      extension === 'json' ? 'json' : 
                      extension === 'md' ? 'markdown' : 'plaintext'
      setOpenTabs([...openTabs, { name: fileName, language }])
    }
    setActiveTab(fileName)
  }

  const handleCloseTab = (fileName: string, e: React.MouseEvent) => {
    e.stopPropagation()
    const newTabs = openTabs.filter(t => t.name !== fileName)
    setOpenTabs(newTabs)
    if (activeTab === fileName && newTabs.length > 0) {
      setActiveTab(newTabs[newTabs.length - 1].name)
    }
  }

  const handleCopy = () => {
    const content = fileContents[activeTab] || ""
    navigator.clipboard.writeText(content)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  const handleRun = () => {
    setIsRunning(true)
    setActivePanel("terminal")
    setShowTerminal(true)
    
    const newOutput = [
      ...terminalOutput.slice(0, -1),
      `>>> python src/${activeTab}`,
      "Running SIR model simulation...",
    ]
    setTerminalOutput(newOutput)

    setTimeout(() => {
      setTerminalOutput([
        ...newOutput,
        "R₀ = 3.00",
        "Peak infection: Day 47 (268 infected)",
        "Final epidemic size: 941 individuals",
        "",
        "Execution completed in 0.042s",
        ">>> "
      ])
      setIsRunning(false)
    }, 1500)
  }

  const handleTerminalInput = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && terminalInput.trim()) {
      const newOutput = [...terminalOutput.slice(0, -1), `>>> ${terminalInput}`]
      
      if (terminalInput.includes("python") || terminalInput.includes("run")) {
        setTerminalOutput([...newOutput, "Running...", ">>> "])
        setTimeout(() => {
          setTerminalOutput([
            ...newOutput,
            "R₀ = 3.00",
            ">>> "
          ])
        }, 500)
      } else if (terminalInput === "clear") {
        setTerminalOutput([">>> "])
      } else {
        setTerminalOutput([...newOutput, ">>> "])
      }
      setTerminalInput("")
    }
  }

  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight
    }
  }, [terminalOutput])

  // Set up Monaco/Yjs binding when the editor or active file changes.
  useEffect(() => {
    const editor = editorRef.current
    if (!editorReady || !editor || !activeTab) return

    bindingsRef.current.forEach((binding) => binding.destroy())
    bindingsRef.current.clear()

    const syncFileId = UUID_RE.test(activeTab) ? activeTab : null
    if (!syncFileId) {
      editor.setValue(fileContents[activeTab] || "// File not found")
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
      yText.insert(0, fileContents[activeTab] || "")
    } else {
      editor.setValue(yText.toString())
    }

    const binding = new MonacoBinding(
      yText,
      editor.getModel()!,
      new Set([editor]),
      undefined
    )
    bindingsRef.current.set(syncFileId, binding)

    return () => {
      binding.destroy()
      bindingsRef.current.delete(syncFileId)
    }
  }, [activeTab, editorReady])

  // Cleanup Yjs on unmount
  useEffect(() => {
    return () => {
      bindingsRef.current.forEach(b => b.destroy())
      yDocsRef.current.forEach(({ provider }) => provider.destroy())
    }
  }, [])

  const currentContent = fileContents[activeTab] || "// File not found"
  const currentLanguage = openTabs.find(t => t.name === activeTab)?.language || "plaintext"

  const handleEditorMount = (editor: monacoEditor.IStandaloneCodeEditor, _monaco: typeof import("monaco-editor")) => {
    editorRef.current = editor
    setEditorReady(true)
  }

  return (
    <div className="h-full flex flex-col bg-[#1e1e1e] border-l border-[#3c3c3c]">
      {/* Title Bar */}
      <div className="h-8 bg-[#323233] flex items-center justify-between px-2 border-b border-[#3c3c3c]">
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5 px-2 py-0.5 rounded bg-primary/10 text-primary text-xs">
            <CheckCircle2 className="w-3 h-3" />
            <span>Docker Ready</span>
          </div>
          <span className="text-xs text-muted-foreground">sir-model - Modeler AI</span>
        </div>
        <div className="flex items-center gap-1">
          <Button variant="ghost" size="icon" className="h-6 w-6 text-muted-foreground hover:text-foreground">
            <Minimize2 className="w-3.5 h-3.5" />
          </Button>
          <Button variant="ghost" size="icon" className="h-6 w-6 text-muted-foreground hover:text-foreground">
            <Maximize2 className="w-3.5 h-3.5" />
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
          <div className="w-56 bg-[#252526] border-r border-[#3c3c3c] flex flex-col">
            <div className="h-9 px-4 flex items-center justify-between border-b border-[#3c3c3c]">
              <span className="text-[11px] font-medium text-[#bbbbbb] uppercase tracking-wider">Explorer</span>
              <Button variant="ghost" size="icon" className="h-5 w-5 text-muted-foreground hover:text-foreground">
                <MoreHorizontal className="w-4 h-4" />
              </Button>
            </div>
            <ScrollArea className="flex-1">
              <div className="py-1">
                <FileTreeItem 
                  item={fileSystem} 
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
                key={tab.name}
                onClick={() => setActiveTab(tab.name)}
                className={cn(
                  "h-full flex items-center gap-2 px-3 cursor-pointer border-r border-[#3c3c3c] group min-w-0",
                  activeTab === tab.name 
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
                  onClick={(e) => handleCloseTab(tab.name, e)}
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
                className="h-6 w-6 text-muted-foreground hover:text-foreground"
              >
                <Split className="w-3.5 h-3.5" />
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
          <div className="h-6 bg-[#1e1e1e] flex items-center px-3 text-xs text-muted-foreground border-b border-[#3c3c3c]">
            <span>sir-model</span>
            <ChevronRight className="w-3 h-3 mx-1" />
            <span>src</span>
            <ChevronRight className="w-3 h-3 mx-1" />
            <span className="text-foreground">{activeTab}</span>
          </div>

          {/* Editor + Terminal */}
          <div className="flex-1 flex flex-col min-h-0">
            {/* Monaco Editor */}
            <div className={cn("flex-1 min-h-0", showTerminal && "h-[60%]")}>
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

            {/* Terminal Panel */}
            {showTerminal && (
              <div className="h-[40%] min-h-[120px] border-t border-[#3c3c3c] flex flex-col bg-[#1e1e1e]">
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
                {activePanel === "terminal" && (
                  <div className="flex-1 flex flex-col min-h-0">
                    <div className="h-7 flex items-center gap-2 px-3 bg-[#1e1e1e] border-b border-[#3c3c3c]">
                      <TerminalIcon className="w-3.5 h-3.5 text-muted-foreground" />
                      <span className="text-xs text-muted-foreground">bash</span>
                      <span className="text-xs text-muted-foreground">- python</span>
                    </div>
                    <ScrollArea className="flex-1" ref={terminalRef}>
                      <div className="p-2 font-mono text-sm">
                        {terminalOutput.map((line, i) => (
                          <div 
                            key={i} 
                            className={cn(
                              "leading-5",
                              line.startsWith(">>>") ? "text-[#4ec9b0]" : 
                              line.includes("Error") ? "text-red-400" :
                              line.includes("completed") ? "text-primary" :
                              "text-[#cccccc]"
                            )}
                          >
                            {line}
                          </div>
                        ))}
                        <div className="flex items-center">
                          <input
                            type="text"
                            value={terminalInput}
                            onChange={(e) => setTerminalInput(e.target.value)}
                            onKeyDown={handleTerminalInput}
                            className="flex-1 bg-transparent text-[#cccccc] outline-none font-mono text-sm"
                            placeholder=""
                          />
                        </div>
                      </div>
                    </ScrollArea>
                  </div>
                )}

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
