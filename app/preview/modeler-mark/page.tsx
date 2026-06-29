"use client"

import { ModelerMark, type ModelerMarkState } from "@/components/chat/modeler-mark"

/**
 * Visual preview for the Modeler AI chat-agent mark.
 *
 * Route: /preview/modeler-mark — open this in the dev build to see
 * the SVG with its morph animation + breathing pulse + state
 * variants. No backend calls; static page.
 *
 * Shows:
 *   1. Multiple sizes (16, 24, 32, 48, 64, 96px) so we can sanity-
 *      check legibility at the sizes we'll actually render.
 *   2. All four `state` variants (idle, thinking, speaking, error)
 *      at a fixed size so the color/animation tweaks are easy
 *      to compare side-by-side.
 *   3. A "before / after" comparison against the old Claude mark
 *      so we can see what the user used to see vs what's coming.
 */

const SIZES = [16, 24, 32, 48, 64, 96] as const
const STATES: ModelerMarkState[] = ["idle", "thinking", "speaking", "error"]

export default function ModelerMarkPreviewPage() {
  return (
    <main className="min-h-screen bg-essay-bg text-essay-text">
      <div className="mx-auto max-w-5xl px-8 py-12">
        <header className="mb-12 border-b border-essay-border pb-8">
          <h1 className="text-3xl font-semibold tracking-tight">
            ModelerMark — preview
          </h1>
          <p className="mt-3 text-essay-text-muted">
            Four mathematical curves (sin → cos → parabola → cubic) morph
            into each other on a 6s loop. Outer breathing pulse is
            driven by <code>--essay-accent</code>. SVG SMIL handles the
            curve morph; CSS handles the breathe.
          </p>
        </header>

        {/* ── Section 1: sizes ───────────────────────────── */}
        <section className="mb-16">
          <h2 className="mb-4 text-xs font-semibold uppercase tracking-wider text-essay-text-faint">
            Sizes
          </h2>
          <div className="flex items-end gap-8 rounded-lg border border-essay-border bg-essay-bg-sidebar p-6">
            {SIZES.map((s) => (
              <div key={s} className="flex flex-col items-center gap-2">
                <ModelerMark size={s} state="thinking" />
                <span className="text-xs text-essay-text-faint">{s}px</span>
              </div>
            ))}
          </div>
        </section>

        {/* ── Section 2: states ──────────────────────────── */}
        <section className="mb-16">
          <h2 className="mb-4 text-xs font-semibold uppercase tracking-wider text-essay-text-faint">
            States
          </h2>
          <div className="grid grid-cols-4 gap-4">
            {STATES.map((state) => (
              <div
                key={state}
                className="flex flex-col items-center gap-3 rounded-lg border border-essay-border bg-essay-bg-sidebar p-6"
              >
                <ModelerMark size={64} state={state} />
                <span className="text-sm font-medium">{state}</span>
                <span className="text-xs text-essay-text-faint text-center">
                  {STATE_DESCRIPTIONS[state]}
                </span>
              </div>
            ))}
          </div>
        </section>

        {/* ── Section 3: before / after ──────────────────── */}
        <section className="mb-16">
          <h2 className="mb-4 text-xs font-semibold uppercase tracking-wider text-essay-text-faint">
            Before / After
          </h2>
          <div className="grid grid-cols-2 gap-4">
            <div className="flex flex-col items-center gap-3 rounded-lg border border-essay-border bg-essay-bg-sidebar p-8">
              <img
                src="/claude-color.svg"
                width={64}
                height={64}
                alt="Old Claude mark"
              />
              <span className="text-sm font-medium">Before — Claude spark</span>
              <span className="text-xs text-essay-text-faint text-center">
                Static SVG. Brand-neutral. Used everywhere.
              </span>
            </div>
            <div className="flex flex-col items-center gap-3 rounded-lg border border-essay-border bg-essay-bg-sidebar p-8">
              <ModelerMark size={64} state="thinking" />
              <span className="text-sm font-medium">After — Modeler mark</span>
              <span className="text-xs text-essay-text-faint text-center">
                Dynamic. Domain-specific. Identity cue.
              </span>
            </div>
          </div>
        </section>

        {/* ── Section 4: in context (next to a message) ──── */}
        <section className="mb-16">
          <h2 className="mb-4 text-xs font-semibold uppercase tracking-wider text-essay-text-faint">
            In context — chat bubble
          </h2>
          <div className="rounded-lg border border-essay-border bg-essay-bg-sidebar p-6">
            <div className="flex items-start gap-3">
              <ModelerMark size={28} state="speaking" className="mt-0.5" />
              <div className="flex-1">
                <div className="mb-1 text-xs font-medium text-essay-text-muted">
                  Modeler AI
                </div>
                <div className="rounded-lg bg-essay-bg px-4 py-3 text-sm text-essay-text leading-relaxed">
                  The gradient of the loss surface flattens near the
                  minimum — try a smaller learning rate around
                  <code className="mx-1 rounded bg-essay-code-bg px-1.5 py-0.5 text-essay-accent">x = 0.42</code>.
                </div>
              </div>
            </div>
          </div>
        </section>

        <footer className="border-t border-essay-border pt-6 text-xs text-essay-text-faint">
          <p>
            Files touched in this preview (no production swaps yet):
            <code className="ml-2 rounded bg-essay-code-bg px-1.5 py-0.5">public/modeler-mark.svg</code>
            <code className="ml-2 rounded bg-essay-code-bg px-1.5 py-0.5">components/chat/modeler-mark.tsx</code>
            <code className="ml-2 rounded bg-essay-code-bg px-1.5 py-0.5">app/preview/modeler-mark/page.tsx</code>
          </p>
        </footer>
      </div>
    </main>
  )
}

const STATE_DESCRIPTIONS: Record<ModelerMarkState, string> = {
  idle: "Dimmed; slower pulse. Waiting on user input.",
  thinking: "Default. Medium pulse, full color.",
  speaking: "Faster pulse while the agent streams a response.",
  error: "Red tint. Same morph; signals failure without animation change.",
}