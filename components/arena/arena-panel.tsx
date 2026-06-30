"use client"

import { useEffect, useMemo, useRef, useState } from "react"
import { BookOpen, FilePlus2, Link2, Loader2, MessageSquare, PanelRightClose, RefreshCw, Save, Sigma, UserRound } from "lucide-react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import remarkMath from "remark-math"
import rehypeKatex from "rehype-katex"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import {
  createArenaCard,
  getArenaIndex,
  updateArenaCard,
  type ArenaCard,
  type ArenaIndex,
  type ProjectCapability,
} from "@/lib/api"
import { ArenaChat } from "@/components/arena/arena-chat"
import { EquationEditor } from "@/components/arena/equation-editor"
import { UserAvatar } from "@/components/ui/user-avatar"
import { useProjectMembers } from "@/hooks/use-project-members"
import { formatRelativeTime } from "@/lib/relative-time"

type ArenaCardType = "formula" | "finding" | "assumption" | "decision"

const NEW_CARD_ACTIONS: Array<{ type: ArenaCardType; label: string }> = [
  { type: "formula", label: "New Formula" },
  { type: "finding", label: "New Finding" },
  { type: "assumption", label: "New Assumption" },
  { type: "decision", label: "New Decision" },
]

const CARD_TYPE_TONE: Record<string, string> = {
  formula: "border-[#64b5f6] text-[#9fd0ff]",
  finding: "border-[#9bd6b5] text-[#b6edcb]",
  assumption: "border-[#d4a574] text-[#ebc396]",
  decision: "border-[#f87171] text-[#ffb4a8]",
}

function defaultBody(cardType: ArenaCardType, title: string) {
  if (cardType === "formula") {
    return `# ${title}\n\n$$\n\n$$\n\nRelated: [[]]\n`
  }
  if (cardType === "decision") {
    return `# ${title}\n\n- Decision:\n- Reason:\n- Impact:\n`
  }
  if (cardType === "assumption") {
    return `# ${title}\n\n- Assumption:\n- Evidence:\n- Risk:\n`
  }
  return `# ${title}\n\n- Finding:\n- Evidence:\n- Related: [[]]\n`
}

function MarkdownPreview({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeKatex]}
      components={{
        h1({ children }) { return <h1 className="mb-3 text-xl font-semibold">{children}</h1> },
        h2({ children }) { return <h2 className="mb-2 mt-4 text-lg font-semibold">{children}</h2> },
        h3({ children }) { return <h3 className="mb-1.5 mt-3 text-base font-semibold">{children}</h3> },
        p({ children }) { return <p className="mb-2 leading-relaxed text-[#d8d8d8]">{children}</p> },
        ul({ children }) { return <ul className="mb-3 list-disc space-y-1 pl-5 text-[#d8d8d8]">{children}</ul> },
        ol({ children }) { return <ol className="mb-3 list-decimal space-y-1 pl-5 text-[#d8d8d8]">{children}</ol> },
        code({ children }) {
          return <code className="rounded bg-[#232323] px-1.5 py-0.5 text-[13px] text-[#ebc396]">{children}</code>
        },
        blockquote({ children }) {
          return <blockquote className="my-2 border-l-2 border-[#d4a574] bg-[#161616] px-3 py-1 text-[#b4b4b4]">{children}</blockquote>
        },
      }}
    >
      {content}
    </ReactMarkdown>
  )
}

// One Authorship row: avatar + name + relative time.
// Used twice in the Info tab (Created by, Last edited by).
function AuthorshipRow({
  userId,
  displayNameByUserId,
  label,
  updatedAt,
}: {
  userId: string | null
  displayNameByUserId: Map<string, string>
  label: string
  updatedAt: number
}) {
  const name = userId ? displayNameByUserId.get(userId) ?? userId : "Unknown"
  return (
    <div className="flex items-center gap-2 py-1">
      <UserAvatar userId={userId} size="md" />
      <div className="min-w-0 flex-1">
        <div className="text-[11px] uppercase tracking-wide text-[#787878]">{label}</div>
        <div className="truncate text-xs text-[#e8e8e8]">
          {name} <span className="text-[#787878]">· {formatRelativeTime(updatedAt)}</span>
        </div>
      </div>
    </div>
  )
}

// Collapses two rows into one when creator == last editor AND no
// separate editor is recorded — i.e. the card was just created and
// nobody has saved over it yet. Once `update_card` fires it overwrites
// `last_edited_by`, so equality here is a reliable signal of
// "freshly created, never edited separately".
function AuthorshipRows({
  selected,
  displayNameByUserId,
}: {
  selected: ArenaCard
  displayNameByUserId: Map<string, string>
}) {
  const sameAuthor =
    !!selected.created_by &&
    !!selected.last_edited_by &&
    selected.created_by === selected.last_edited_by
  if (sameAuthor) {
    return (
      <AuthorshipRow
        userId={selected.created_by}
        displayNameByUserId={displayNameByUserId}
        label="Created by"
        updatedAt={selected.updated_at}
      />
    )
  }
  return (
    <>
      <AuthorshipRow
        userId={selected.created_by}
        displayNameByUserId={displayNameByUserId}
        label="Created by"
        updatedAt={selected.updated_at}
      />
      <AuthorshipRow
        userId={selected.last_edited_by}
        displayNameByUserId={displayNameByUserId}
        label="Last edited by"
        updatedAt={selected.updated_at}
      />
    </>
  )
}

export function ArenaPanel({
  projectId,
  capabilities = [],
}: {
  projectId: string
  capabilities?: ProjectCapability[]
}) {
  const [index, setIndex] = useState<ArenaIndex>({ cards: [], unresolved_links: [] })
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [draft, setDraft] = useState("")
  const [mode, setMode] = useState<"edit" | "preview">("edit")
  const [sidebarTab, setSidebarTab] = useState<"info" | "chat">("info")
  const [status, setStatus] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [equationOpen, setEquationOpen] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const canWrite = capabilities.includes("files.write")
  const selected = useMemo(
    () => index.cards.find((card) => card.file_id === selectedId) || index.cards[0] || null,
    [index.cards, selectedId],
  )

  // Members lookup → displayNameByUserId Map for the avatar / byline.
  // The hook fetches /projects/<pid>/members once on mount and re-fetches
  // when projectId changes; we don't currently refresh after card edits
  // since the members list itself doesn't change as a side effect of
  // writing cards.
  const { displayNameByUserId } = useProjectMembers(projectId)

  useEffect(() => {
    if (selected) {
      setSelectedId(selected.file_id)
      setDraft(selected.content)
    } else {
      setDraft("")
    }
  }, [selected?.file_id])

  const refresh = async (nextSelectedId?: string) => {
    setLoading(true)
    setStatus(null)
    try {
      const next = await getArenaIndex(projectId)
      setIndex(next)
      if (nextSelectedId) setSelectedId(nextSelectedId)
      else if (!selectedId && next.cards[0]) setSelectedId(next.cards[0].file_id)
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Arena unavailable.")
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void refresh()
  }, [projectId])

  const createCard = async (cardType: ArenaCardType) => {
    if (!canWrite) return
    const title = window.prompt("Title")
    if (!title?.trim()) return
    setSaving(true)
    setStatus(null)
    try {
      const card = await createArenaCard(projectId, {
        card_type: cardType,
        title: title.trim(),
        tags: [cardType],
        body: defaultBody(cardType, title.trim()),
      })
      await refresh(card.file_id)
      setMode("edit")
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Create failed.")
    } finally {
      setSaving(false)
    }
  }

  const saveCard = async () => {
    if (!selected || !canWrite) return
    setSaving(true)
    setStatus(null)
    try {
      const card = await updateArenaCard(projectId, selected.file_id, {
        content: draft,
        expected_updated_at: selected.updated_at,
      })
      await refresh(card.file_id)
      setStatus("Saved.")
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Save failed.")
    } finally {
      setSaving(false)
    }
  }

  // Insert LaTeX from the equation editor at the textarea caret (or append).
  const insertEquation = (latex: string, eqDisplay: "block" | "inline") => {
    const snippet = eqDisplay === "block" ? `\n$$\n${latex}\n$$\n` : `$${latex}$`
    const textarea = textareaRef.current
    if (!textarea) {
      setDraft((prev) => prev + snippet)
      return
    }
    const start = textarea.selectionStart ?? draft.length
    const end = textarea.selectionEnd ?? draft.length
    const next = draft.slice(0, start) + snippet + draft.slice(end)
    setDraft(next)
    // Restore caret just after the inserted snippet on next tick.
    requestAnimationFrame(() => {
      const caret = start + snippet.length
      textarea.focus()
      textarea.setSelectionRange(caret, caret)
    })
  }

  return (
    <section className="grid h-full min-h-0 grid-cols-[260px_minmax(0,1fr)_260px] bg-[#0d0d0d] text-[#e8e8e8]">
      <aside className="flex min-h-0 flex-col border-r border-[#373737] bg-[#151515]">
        <div className="flex h-10 items-center gap-2 border-b border-[#373737] px-3">
          <BookOpen className="h-4 w-4 text-[#d4a574]" />
          <span className="text-xs font-medium uppercase text-[#b4b4b4]">Arena</span>
          <button className="ml-auto text-[#787878] hover:text-[#e8e8e8]" onClick={() => refresh()} title="Refresh">
            <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
          </button>
        </div>
        <div className="grid gap-1 border-b border-[#373737] p-2">
          {NEW_CARD_ACTIONS.map((action) => (
            <button
              key={action.type}
              disabled={!canWrite || saving}
              onClick={() => createCard(action.type)}
              className="flex h-8 items-center gap-2 rounded-md border border-[#373737] bg-[#1f1f1f] px-2 text-left text-xs text-[#b4b4b4] hover:border-[#d4a574] hover:text-[#e8e8e8] disabled:cursor-not-allowed disabled:text-[#5f5f5f]"
            >
              <FilePlus2 className="h-3.5 w-3.5 text-[#d4a574]" />
              {action.label}
            </button>
          ))}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto py-1">
          {index.cards.length === 0 ? (
            <div className="px-3 py-6 text-center text-xs text-[#787878]">No Arena cards.</div>
          ) : index.cards.map((card) => {
            // Recency signal: who last touched this card. Fall back to the
            // creator so legacy cards (last_edited_by = null) still show
            // *some* author rather than "Unknown".
            const editorId = card.last_edited_by ?? card.created_by
            const editorName = editorId ? displayNameByUserId.get(editorId) ?? null : null
            return (
            <button
              key={card.file_id}
              onClick={() => { setSelectedId(card.file_id); setDraft(card.content) }}
              className={cn(
                "w-full border-l-2 px-3 py-2 text-left text-xs hover:bg-[#232323]",
                selected?.file_id === card.file_id ? "border-[#d4a574] bg-[#202020]" : "border-transparent",
              )}
            >
              <div className="flex items-center gap-2">
                <span className={cn("rounded border px-1.5 py-0.5 text-[10px]", CARD_TYPE_TONE[card.card_type] || "border-[#787878] text-[#b4b4b4]")}>{card.card_type}</span>
                <span className="min-w-0 flex-1 truncate text-[#e8e8e8]">{card.title}</span>
                <UserAvatar userId={editorId} name={editorName} size="sm" />
              </div>
              <div className="mt-1 truncate text-[11px] text-[#787878]">{card.tags.map((tag) => `#${tag}`).join(" ")}</div>
            </button>
            )
          })}
        </div>
      </aside>

      <main className="flex min-h-0 min-w-0 flex-col">
        <div className="flex min-h-10 flex-col gap-0.5 border-b border-[#373737] bg-[#121212] px-3 py-1.5">
          <div className="flex w-full items-center gap-2">
            <span className="min-w-0 flex-1 truncate text-sm font-medium">{selected?.title || "Arena"}</span>
            <div className="flex overflow-hidden rounded-md border border-[#373737] bg-[#1a1a1a]">
              {(["edit", "preview"] as const).map((item) => (
                <button
                  key={item}
                  onClick={() => setMode(item)}
                  className={cn(
                    "px-2.5 py-1 text-xs capitalize text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8]",
                    mode === item && "bg-[#232323] text-[#e8e8e8]",
                  )}
                >
                  {item}
                </button>
              ))}
            </div>
            {mode === "edit" && canWrite && selected && (
              <Button
                variant="ghost"
                onClick={() => setEquationOpen(true)}
                title="Insert equation"
                className="h-7 gap-1.5 border border-[#373737] px-2 text-xs text-[#9fd0ff] hover:border-[#64b5f6] hover:bg-[#232323]"
              >
                <Sigma className="h-3.5 w-3.5" />
                Equation
              </Button>
            )}
            <Button
              onClick={saveCard}
              disabled={!selected || !canWrite || saving || draft === selected.content}
              className="h-7 bg-[#d4a574] px-2 text-xs text-[#111111] hover:bg-[#ebc396]"
            >
              {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Save className="h-3.5 w-3.5" />}
            </Button>
          </div>
          {/* Authorship byline: "Edited by X · 2h ago" (or just the creator
              if last_edited_by hasn't been set — covers the freshly-created
              card case where both fields are equal). */}
          {selected && (() => {
            const editorId = selected.last_edited_by ?? selected.created_by
            if (!editorId) {
              return (
                <div className="flex items-center gap-1.5 text-[11px] text-[#787878]">
                  <UserAvatar userId={null} size="md" />
                  <span>Edited by Unknown · {formatRelativeTime(selected.updated_at)}</span>
                </div>
              )
            }
            const editorName = displayNameByUserId.get(editorId) ?? null
            return (
              <div className="flex items-center gap-1.5 text-[11px] text-[#787878]">
                <UserAvatar userId={editorId} name={editorName} size="md" />
                <span>Edited by {editorName ?? editorId} · {formatRelativeTime(selected.updated_at)}</span>
              </div>
            )
          })()}
        </div>
        {status && (
          <div className="border-b border-[#373737] bg-[#181818] px-3 py-2 text-xs text-[#b4b4b4]">{status}</div>
        )}
        <div className="min-h-0 flex-1">
          {selected ? (
            mode === "edit" ? (
              <Textarea
                ref={textareaRef}
                value={draft}
                readOnly={!canWrite}
                onChange={(event) => setDraft(event.target.value)}
                className="h-full min-h-full resize-none rounded-none border-0 bg-[#0d0d0d] p-4 font-mono text-sm leading-6 text-[#e8e8e8] shadow-none focus-visible:ring-0"
              />
            ) : (
              <div className="h-full overflow-y-auto px-5 py-4 text-sm">
                <MarkdownPreview content={draft} />
              </div>
            )
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-[#787878]">Arena</div>
          )}
        </div>
      </main>

      <aside className="flex min-h-0 flex-col border-l border-[#373737] bg-[#151515]">
        {/* Tab bar */}
        <div className="flex border-b border-[#373737] bg-[#121212]">
          {([
            { key: "info" as const, label: "Info", Icon: Link2 },
            { key: "chat" as const, label: "Chat", Icon: MessageSquare },
          ]).map(({ key, label, Icon }) => (
            <button
              key={key}
              onClick={() => setSidebarTab(key)}
              className={cn(
                "flex flex-1 items-center justify-center gap-1.5 py-2 text-xs font-medium uppercase transition-colors",
                sidebarTab === key
                  ? "border-b-2 border-[#d4a574] text-[#e8e8e8] bg-[#1a1a1a]"
                  : "text-[#787878] hover:text-[#b4b4b4] hover:bg-[#181818]",
              )}
            >
              <Icon className="h-3.5 w-3.5" />
              {label}
            </button>
          ))}
        </div>

        {sidebarTab === "info" ? (
          <>
            <section className="border-b border-[#373737] p-3">
              <div className="mb-2 flex items-center gap-2 text-xs font-medium uppercase text-[#b4b4b4]">
                <Link2 className="h-3.5 w-3.5 text-[#d4a574]" />
                Backlinks
              </div>
              <div className="space-y-1">
                {(selected?.backlinks || []).length === 0 ? (
                  <div className="text-xs text-[#787878]">None</div>
                ) : selected?.backlinks.map((title) => (
                  <button key={title} className="block w-full truncate rounded px-2 py-1 text-left text-xs text-[#b4b4b4] hover:bg-[#232323]">
                    {title}
                  </button>
                ))}
              </div>
            </section>
            {/* Authorship: two rows (Created by, Last edited by) with a
                32px avatar + name + relative time. Both rows collapse to
                a single line when the timestamps and user_ids match
                (freshly-created card that hasn't been edited yet). */}
            <section className="border-b border-[#373737] p-3">
              <div className="mb-2 flex items-center gap-2 text-xs font-medium uppercase text-[#b4b4b4]">
                <UserRound className="h-3.5 w-3.5 text-[#d4a574]" />
                Authorship
              </div>
              {selected ? (
                <AuthorshipRows
                  selected={selected}
                  displayNameByUserId={displayNameByUserId}
                />
              ) : (
                <div className="text-xs text-[#787878]">Select a card to view authorship.</div>
              )}
            </section>
            <section className="border-b border-[#373737] p-3">
              <div className="mb-2 text-xs font-medium uppercase text-[#b4b4b4]">Unresolved</div>
              <div className="flex flex-wrap gap-1">
                {(selected?.unresolved_links || index.unresolved_links).length === 0 ? (
                  <span className="text-xs text-[#787878]">None</span>
                ) : (selected?.unresolved_links || index.unresolved_links).map((link) => (
                  <span key={link} className="rounded border border-[#5f3f24] px-1.5 py-0.5 text-[11px] text-[#ebc396]">
                    {link}
                  </span>
                ))}
              </div>
            </section>
          </>
        ) : (
          <div className="min-h-0 flex-1">
            <ArenaChat projectId={projectId} capabilities={capabilities} />
          </div>
        )}
      </aside>

      <EquationEditor
        open={equationOpen}
        onOpenChange={setEquationOpen}
        onInsert={insertEquation}
      />
    </section>
  )
}
