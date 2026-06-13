"use client"

import { useState } from "react"
import dynamic from "next/dynamic"
import { Sigma } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

// MathField pulls in the mathlive bundle; keep it out of the SSR/initial chunk.
const MathField = dynamic(
  () => import("@/components/arena/math-field").then((mod) => mod.MathField),
  { ssr: false, loading: () => <div className="h-12 animate-pulse rounded bg-[#1a1a1a]" /> },
)

// Quick-insert templates. `$0` marks where MathLive should drop the caret,
// but for simplicity we just seed the field — users edit visually after.
const PRESETS: Array<{ label: string; latex: string }> = [
  { label: "Fraction", latex: "\\frac{a}{b}" },
  { label: "Square root", latex: "\\sqrt{x}" },
  { label: "Power", latex: "x^{n}" },
  { label: "Subscript", latex: "x_{i}" },
  { label: "Sum", latex: "\\sum_{i=1}^{n} x_i" },
  { label: "Integral", latex: "\\int_{a}^{b} f(x)\\,dx" },
  { label: "Limit", latex: "\\lim_{x \\to \\infty} f(x)" },
  { label: "Matrix", latex: "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}" },
  { label: "Cases", latex: "f(x) = \\begin{cases} a & x > 0 \\\\ b & x \\le 0 \\end{cases}" },
  { label: "Vector", latex: "\\vec{v}" },
  { label: "Partial", latex: "\\frac{\\partial f}{\\partial x}" },
  { label: "Greek", latex: "\\alpha \\beta \\gamma" },
]

export function EquationEditor({
  open,
  onOpenChange,
  onInsert,
  display = "block",
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onInsert: (latex: string, display: "block" | "inline") => void
  display?: "block" | "inline"
}) {
  const [latex, setLatex] = useState("")
  const [mode, setMode] = useState<"block" | "inline">(display)

  const handleInsert = () => {
    const value = latex.trim()
    if (!value) return
    onInsert(value, mode)
    setLatex("")
    onOpenChange(false)
  }

  const seedPreset = (preset: string) => {
    setLatex((prev) => (prev.trim() ? `${prev} ${preset}` : preset))
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl border-[#373737] bg-[#151515] text-[#e8e8e8]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sigma className="h-4 w-4 text-[#64b5f6]" />
            Equation Editor
          </DialogTitle>
        </DialogHeader>

        <div className="grid gap-3">
          {/* Visual editor */}
          <div className="rounded-lg border border-[#373737] bg-[#0d0d0d] p-3">
            <MathField
              value={latex}
              onChange={setLatex}
              onEnter={handleInsert}
              autoFocus
              className="cc-mathfield"
            />
          </div>

          {/* Preset palette */}
          <div className="flex flex-wrap gap-1.5">
            {PRESETS.map((preset) => (
              <button
                key={preset.label}
                type="button"
                onClick={() => seedPreset(preset.latex)}
                className="rounded-md border border-[#373737] bg-[#1a1a1a] px-2 py-1 text-xs text-[#b4b4b4] hover:border-[#64b5f6] hover:text-[#e8e8e8]"
              >
                {preset.label}
              </button>
            ))}
          </div>

          {/* Raw LaTeX (for power users) */}
          <div>
            <div className="mb-1 text-[11px] uppercase tracking-wide text-[#787878]">LaTeX</div>
            <textarea
              value={latex}
              onChange={(event) => setLatex(event.target.value)}
              placeholder="\frac{a}{b}"
              className="h-16 w-full resize-none rounded-md border border-[#373737] bg-[#0d0d0d] p-2 font-mono text-xs text-[#9fd0ff] focus:border-[#64b5f6] focus:outline-none"
            />
          </div>

          {/* Block vs inline */}
          <div className="flex items-center gap-2 text-xs">
            <span className="text-[#787878]">Insert as</span>
            <div className="flex overflow-hidden rounded-md border border-[#373737]">
              {(["block", "inline"] as const).map((item) => (
                <button
                  key={item}
                  type="button"
                  onClick={() => setMode(item)}
                  className={cn(
                    "px-2.5 py-1 capitalize text-[#787878] hover:bg-[#232323]",
                    mode === item && "bg-[#232323] text-[#e8e8e8]",
                  )}
                >
                  {item}
                </button>
              ))}
            </div>
          </div>
        </div>

        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => onOpenChange(false)}
            className="text-[#b4b4b4] hover:bg-[#232323] hover:text-[#e8e8e8]"
          >
            Cancel
          </Button>
          <Button
            onClick={handleInsert}
            disabled={!latex.trim()}
            className="bg-[#64b5f6] text-[#06121f] hover:bg-[#8ecbff]"
          >
            Insert
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
