"use client"

import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter"
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism"

// Renders [n] citations as clickable superscript links to #ref-n anchors.
function CitationLink({ num }: { num: number }) {
  return (
    <sup>
      <a
        href={`#ref-${num}`}
        className="inline-flex items-center justify-center rounded-full border border-[#373737] bg-[#1a1a1a] px-1 text-[10px] font-medium text-[#d4a574] no-underline hover:bg-[#2a2a2a]"
      >
        {num}
      </a>
    </sup>
  )
}

/**
 * Shared dark-theme markdown renderer with citation linking, code blocks with
 * copy buttons, and GFM. Used by both the AI search page and the agentic
 * research view so citation/styling behavior stays consistent.
 */
export function ResearchMarkdown({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        code({ className, children, ...props }) {
          const match = /language-(\w+)/.exec(className || "")
          const code = String(children).replace(/\n$/, "")
          const isInline = !match && !code.includes("\n")
          if (isInline) {
            return (
              <code className="rounded bg-[#232323] px-1.5 py-0.5 text-[13px] text-[#d4a574]" {...props}>
                {children}
              </code>
            )
          }
          return (
            <div className="my-2 overflow-hidden rounded-lg border border-[#373737]">
              <div className="flex items-center justify-between bg-[#1e1e1e] px-3 py-1.5 text-xs text-[#787878]">
                <span>{match?.[1] || "text"}</span>
                <button className="hover:text-[#e8e8e8]" onClick={() => navigator.clipboard.writeText(code)}>
                  Copy
                </button>
              </div>
              <SyntaxHighlighter
                language={match?.[1] || "text"}
                style={vscDarkPlus}
                customStyle={{ margin: 0, borderRadius: 0, fontSize: "13px" }}
              >
                {code}
              </SyntaxHighlighter>
            </div>
          )
        },
        sup({ children }) {
          const text = String(children)
          const match = text.match(/^\[(\d+)\]$/)
          if (match) return <CitationLink num={parseInt(match[1]!, 10)} />
          return <sup>{children}</sup>
        },
        a({ href, children }) {
          return (
            <a href={href} target="_blank" rel="noopener noreferrer" className="text-[#d4a574] underline">
              {children}
            </a>
          )
        },
        h1: ({ children }) => <h1 className="mt-4 mb-2 text-xl font-semibold">{children}</h1>,
        h2: ({ children }) => <h2 className="mt-3 mb-1.5 text-lg font-semibold">{children}</h2>,
        h3: ({ children }) => <h3 className="mt-2 mb-1 text-base font-semibold">{children}</h3>,
        p: ({ children }) => <p className="mb-2 leading-relaxed">{children}</p>,
        ul: ({ children }) => <ul className="mb-2 list-disc space-y-1 pl-5">{children}</ul>,
        ol: ({ children }) => <ol className="mb-2 list-decimal space-y-1 pl-5">{children}</ol>,
        blockquote: ({ children }) => (
          <blockquote className="my-2 border-l-2 border-[#d4a574] bg-[#1a1a1a] px-3 py-1 italic text-[#b4b4b4]">
            {children}
          </blockquote>
        ),
      }}
    >
      {content}
    </ReactMarkdown>
  )
}
