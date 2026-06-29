"use client"

import { X } from "lucide-react"
import { cn } from "@/lib/utils"

/**
 * One open tab in the essay editor's tab strip.
 *
 * `id` is the canonical note basename (without `.md`) — we use it to
 * de-duplicate entries so re-opening the same file focuses the
 * existing tab rather than adding a second one.
 */
export interface EssayTab {
  id: string
  /** Display name shown in the tab. Typically the file's basename with
   * `.md` extension. */
  name: string
  /** Set by the editor's updateListener when the doc has unsaved changes
   * vs the last persisted Y.Doc state. Renders a small dot before the
   * tab name. */
  dirty?: boolean
}

interface EssayTabStripProps {
  tabs: EssayTab[]
  activeTabId: string | null
  onSelect: (tabId: string) => void
  onClose: (tabId: string) => void
}

export function EssayTabStrip({
  tabs,
  activeTabId,
  onSelect,
  onClose,
}: EssayTabStripProps) {
  if (tabs.length === 0) return null
  return (
    <div className="essay-tab-strip" role="tablist">
      {tabs.map((tab) => {
        const isActive = tab.id === activeTabId
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={isActive}
            className={cn("essay-tab", isActive && "essay-tab--active")}
            onClick={() => onSelect(tab.id)}
            onAuxClick={(event) => {
              // Middle-click closes — common IDE / Obsidian gesture.
              if (event.button === 1) {
                event.preventDefault()
                onClose(tab.id)
              }
            }}
            title={tab.name}
          >
            {tab.dirty && <span className="essay-tab__dirty" aria-label="unsaved" />}
            <span className="essay-tab__name">{tab.name}</span>
            <button
              className="essay-tab__close"
              aria-label={`Close ${tab.name}`}
              onClick={(event) => {
                event.stopPropagation()
                onClose(tab.id)
              }}
            >
              <X className="h-3 w-3" />
            </button>
          </div>
        )
      })}
    </div>
  )
}