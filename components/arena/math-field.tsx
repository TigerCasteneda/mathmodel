"use client"

import { useEffect, useRef } from "react"

// MathLive registers a <math-field> custom element. We load it client-side
// only (Next.js SSR has no `window`/`customElements`) and bridge its value
// to React via controlled props.

type MathFieldElement = HTMLElement & {
  value: string
  getValue: (format?: string) => string
  setValue: (value: string, options?: { silenceNotifications?: boolean }) => void
}

let mathliveLoad: Promise<typeof import("mathlive")> | null = null

function loadMathlive() {
  if (!mathliveLoad) mathliveLoad = import("mathlive")
  return mathliveLoad
}

export function MathField({
  value,
  onChange,
  onEnter,
  autoFocus = false,
  className,
}: {
  value: string
  onChange: (latex: string) => void
  onEnter?: () => void
  autoFocus?: boolean
  className?: string
}) {
  const hostRef = useRef<HTMLDivElement>(null)
  const fieldRef = useRef<MathFieldElement | null>(null)
  const onChangeRef = useRef(onChange)
  const onEnterRef = useRef(onEnter)
  onChangeRef.current = onChange
  onEnterRef.current = onEnter

  // Mount the web component once.
  useEffect(() => {
    let disposed = false
    void loadMathlive().then(() => {
      if (disposed || !hostRef.current) return
      const field = document.createElement("math-field") as MathFieldElement
      field.setAttribute("class", className || "")
      // Compact, contained virtual keyboard; show on focus.
      field.setAttribute("math-virtual-keyboard-policy", "manual")
      field.value = value
      field.addEventListener("input", () => {
        onChangeRef.current(field.value)
      })
      field.addEventListener("keydown", (event: Event) => {
        const ke = event as KeyboardEvent
        if (ke.key === "Enter" && (ke.metaKey || ke.ctrlKey)) {
          ke.preventDefault()
          onEnterRef.current?.()
        }
      })
      hostRef.current.appendChild(field)
      fieldRef.current = field
      if (autoFocus) field.focus()
    })
    return () => {
      disposed = true
      fieldRef.current?.remove()
      fieldRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Sync external value changes (e.g. preset insert) without losing the caret
  // during normal typing.
  useEffect(() => {
    const field = fieldRef.current
    if (field && field.value !== value) {
      field.setValue(value, { silenceNotifications: true })
    }
  }, [value])

  return <div ref={hostRef} className="cc-mathfield-host" />
}
