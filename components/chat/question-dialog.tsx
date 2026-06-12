"use client"

import { useEffect, useState } from "react"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { cn } from "@/lib/utils"
import {
  onQuestion,
  resolveQuestion,
  type QuestionEvent,
  type QuestionItem,
} from "@/lib/tauri-api"

export function QuestionDialog({
  conversationId,
}: {
  conversationId: string
}) {
  const [event, setEvent] = useState<QuestionEvent | null>(null)
  const [answers, setAnswers] = useState<Record<string, string[]>>({})
  const [resolving, setResolving] = useState(false)

  useEffect(() => {
    return onQuestion((ev) => {
      if (ev.conversation_id === conversationId) {
        setEvent(ev)
        setAnswers({})
      }
    })
  }, [conversationId])

  // Auto-expire
  useEffect(() => {
    if (!event) return
    if (event.expires_at_ms <= Date.now()) {
      setEvent(null)
      return
    }
    const timeout = setTimeout(
      () => setEvent(null),
      event.expires_at_ms - Date.now() + 500,
    )
    return () => clearTimeout(timeout)
  }, [event])

  const toggleOption = (questionText: string, label: string, multi: boolean) => {
    setAnswers((prev) => {
      const current = prev[questionText] || []
      if (multi) {
        return {
          ...prev,
          [questionText]: current.includes(label)
            ? current.filter((l) => l !== label)
            : [...current, label],
        }
      }
      return { ...prev, [questionText]: [label] }
    })
  }

  const handleSubmit = async () => {
    if (!event || resolving) return
    setResolving(true)
    try {
      await resolveQuestion(event.request_id, answers)
      setEvent(null)
    } catch {
      // fallback
    } finally {
      setResolving(false)
    }
  }

  if (!event) return null

  return (
    <AlertDialog open>
      <AlertDialogContent className="max-w-xl border-[#373737] bg-[#151515] text-[#e8e8e8]">
        <AlertDialogHeader>
          <AlertDialogTitle>Decision Needed</AlertDialogTitle>
          <AlertDialogDescription className="text-[#b4b4b4]">
            The AI needs your input to proceed.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="grid gap-4 max-h-[60vh] overflow-y-auto">
          {event.questions.map((q) => (
            <QuestionBlock
              key={q.question}
              item={q}
              selected={answers[q.question] || []}
              onToggle={(label) => toggleOption(q.question, label, q.multiSelect)}
            />
          ))}
        </div>
        <AlertDialogFooter>
          <button
            className="rounded-md border border-[#373737] bg-transparent px-4 py-2 text-sm text-[#b4b4b4] hover:bg-[#232323]"
            onClick={() => setEvent(null)}
          >
            Cancel
          </button>
          <button
            className="rounded-md bg-[#d4a574] px-4 py-2 text-sm text-[#111111] hover:bg-[#ebc396]"
            onClick={handleSubmit}
            disabled={resolving}
          >
            {resolving ? "Submitting..." : "Submit"}
          </button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

function QuestionBlock({
  item,
  selected,
  onToggle,
}: {
  item: QuestionItem
  selected: string[]
  onToggle: (label: string) => void
}) {
  return (
    <div>
      <div className="mb-2 text-xs font-medium uppercase text-[#d4a574]">{item.header}</div>
      <p className="mb-2 text-sm">{item.question}</p>
      <div className="grid gap-1.5">
        {item.options.map((opt) => {
          const isSelected = selected.includes(opt.label)
          return (
            <button
              key={opt.label}
              type="button"
              onClick={() => onToggle(opt.label)}
              className={cn(
                "flex items-center gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors",
                isSelected
                  ? "border-[#d4a574] bg-[#2a2218] text-[#e8e8e8]"
                  : "border-[#373737] bg-[#1a1a1a] text-[#b4b4b4] hover:border-[#555]",
              )}
            >
              <span
                className={cn(
                  "flex h-4 w-4 shrink-0 items-center justify-center rounded border text-xs",
                  isSelected
                    ? "border-[#d4a574] bg-[#d4a574] text-[#111]"
                    : "border-[#555]",
                )}
              >
                {isSelected && "✓"}
              </span>
              <div className="text-left">
                <div className="font-medium">{opt.label}</div>
                {opt.description && (
                  <div className="text-xs text-[#787878]">{opt.description}</div>
                )}
              </div>
            </button>
          )
        })}
      </div>
    </div>
  )
}
