"use client"

import { cn } from "@/lib/utils"

/**
 * ModelerMark — the chat-agent identity icon.
 *
 * Renders the dynamic SVG mark inline so:
 *   - SMIL `<animate>` on the `d` attribute morphs the curve
 *     (sin → cos → parabola → cubic → sin, 6s loop) without JS
 *   - CSS `currentColor` lets the parent tint via the
 *     `.modeler-mark` class or an inline `style.color`
 *   - The wrapper's breathing pulse is driven by `--essay-accent`
 *     and modified by the `state` prop
 *
 * Use `<img src="/modeler-mark.svg" />` only for static contexts
 * (favicons, OG images) where the morph animation isn't useful.
 *
 * @example
 *   <ModelerMark size={32} state="thinking" />
 *   <ModelerMark size={48} state="idle" className="ml-2" />
 */

export type ModelerMarkState = "idle" | "thinking" | "speaking" | "error"

export interface ModelerMarkProps {
  /** Pixel width and height. Default 32. */
  size?: number
  /** Drives the breathing-pulse rate + color. */
  state?: ModelerMarkState
  className?: string
  /** Override the stroke color via inline style; falls back to
   * `--essay-accent`. Useful for one-off tinting in marketing
   * surfaces without a state override. */
  style?: React.CSSProperties
}

export function ModelerMark({
  size = 32,
  state = "thinking",
  className,
  style,
}: ModelerMarkProps) {
  return (
    <span
      role="img"
      aria-label="Modeler AI agent"
      className={cn(
        "modeler-mark",
        `modeler-mark--${state}`,
        className,
      )}
      style={{ width: size, height: size, ...style }}
    >
      <svg
        viewBox="0 0 64 64"
        xmlns="http://www.w3.org/2000/svg"
        fill="none"
        stroke="currentColor"
        strokeWidth={2.5}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {/* baseline tick marks — subtle "this is a chart" cue */}
        <g opacity={0.35} strokeWidth={1.5}>
          <line x1="6" y1="32" x2="10" y2="32" />
          <line x1="54" y1="32" x2="58" y2="32" />
          <line x1="32" y1="6" x2="32" y2="10" />
          <line x1="32" y1="54" x2="32" y2="58" />
        </g>

        {/* morphing curve — sin → cos → parabola → cubic → sin */}
        <path d="M 10 32 C 22 16, 42 48, 54 32">
          <animate
            attributeName="d"
            dur="6s"
            repeatCount="indefinite"
            calcMode="spline"
            keyTimes="0; 0.25; 0.5; 0.75; 1"
            keySplines="0.45 0 0.25 1; 0.45 0 0.25 1; 0.45 0 0.25 1; 0.45 0 0.25 1"
            values="
              M 10 32 C 22 16, 42 48, 54 32;
              M 10 32 C 22 48, 42 16, 54 32;
              M 10 12 C 22 52, 42 52, 54 12;
              M 10 32 C 18 8, 46 56, 54 32;
              M 10 32 C 22 16, 42 48, 54 32
            "
          />
        </path>
      </svg>
    </span>
  )
}