"use client"

import dynamic from "next/dynamic"

const MonacoEditor = dynamic(
  () => import("@monaco-editor/react").then((mod) => mod.default),
  { ssr: false },
)

export function CodeEditor({
  language,
  value,
}: {
  language: string
  value: string
}) {
  return (
    <MonacoEditor
      height="100%"
      language={language}
      theme="vs-dark"
      value={value}
      options={{
        minimap: { enabled: true, scale: 0.8 },
        fontSize: 13,
        lineNumbers: "on",
        scrollBeyondLastLine: false,
        padding: { top: 10, bottom: 10 },
        renderWhitespace: "selection",
        wordWrap: "on",
      }}
    />
  )
}
